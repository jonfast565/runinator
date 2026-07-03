// port of core/utils/wdl-expression.ts.

import 'dart:convert';

import '../domain/json.dart';

enum _TokenKind { ident, string, number, op, punct, eof }

class _Token {
  const _Token(this.kind, this.text);

  final _TokenKind kind;
  final String text;
}

const List<String> _expressionKeys = [
  r'$ref',
  r'$concat',
  r'$coalesce',
  r'$literal',
  r'$to_string',
  r'$to_json_string',
  r'$node',
];

String expressionJsonToWdl(Object? value) {
  if (value == null) {
    return 'null';
  }

  if (value is bool || value is num) {
    return value.toString();
  }

  if (value is String) {
    return _secretRefToWdl(value) ?? _quote(value);
  }

  if (value is List) {
    return '[${value.map(expressionJsonToWdl).join(", ")}]';
  }

  if (!_isRecord(value)) {
    return 'null';
  }

  final record = value as JsonRecord;
  final keys = record.keys.toList();

  if (keys.length == 1) {
    final ref = record[r'$ref'];
    if (_isRecord(ref)) {
      return _refToWdl(ref as JsonRecord);
    }

    final concat = record[r'$concat'];
    if (concat is List) {
      return _joinOperator(concat, ' ++ ');
    }

    final coalesce = record[r'$coalesce'];
    if (coalesce is List) {
      return _joinOperator(coalesce, ' ?? ');
    }

    if (record.containsKey(r'$to_string')) {
      return 'string(${expressionJsonToWdl(record[r'$to_string'])})';
    }

    if (record.containsKey(r'$to_json_string')) {
      return 'json(${expressionJsonToWdl(record[r'$to_json_string'])})';
    }
  }

  final entries = record.entries.map((entry) => '${_objectKey(entry.key)}: ${expressionJsonToWdl(entry.value)}');
  return '{ ${entries.join(", ")} }';
}

JsonValue parseWdlExpression(String source) {
  final parser = _Parser(_tokenize(source));
  final value = parser.parseExpression();
  parser.expect(_TokenKind.eof);
  return value;
}

bool isWorkflowExpressionValue(Object? value) =>
    _isRecord(value) && _expressionKeys.any((key) => (value as JsonRecord).containsKey(key));

String _joinOperator(List<Object?> items, String operator) =>
    items.map((item) => _wrapBinaryPart(expressionJsonToWdl(item))).join(operator);

final RegExp _binaryOpPattern = RegExp(r'\s(?:\+\+|\?\?)\s');

String _wrapBinaryPart(String value) => _binaryOpPattern.hasMatch(value) ? '($value)' : value;

String _refToWdl(JsonRecord ref) {
  if (ref['params'] is List) {
    return _appendPath('params', ref['params'] as List);
  }

  if (ref['prev'] is List) {
    return _appendPath('prev', ref['prev'] as List);
  }

  if (ref['workflow'] is List) {
    return _appendPath('run', ref['workflow'] as List);
  }

  if (ref['config'] is List) {
    return _appendPath('config', ref['config'] as List);
  }

  if (ref['node'] is String && ref['output'] is List) {
    return _appendPath(ref['node'] as String, ref['output'] as List);
  }

  return _objectLiteral(ref);
}

String _appendPath(String head, List<Object?> path) =>
    [head, ...path.map((segment) => segment.toString())].join('.');

String _objectLiteral(JsonRecord record) {
  final entries = record.entries.map((entry) => '${_objectKey(entry.key)}: ${expressionJsonToWdl(entry.value)}');
  return '{ ${entries.join(", ")} }';
}

final RegExp _identPattern = RegExp(r'^[A-Za-z_][A-Za-z0-9_]*$');

String _objectKey(String key) => _identPattern.hasMatch(key) ? key : _quote(key);

String _quote(String value) => jsonEncode(value);

String? _secretRefToWdl(String value) {
  if (!value.startsWith('secret://')) {
    return null;
  }

  final rest = value.substring('secret://'.length);
  final parts = rest.split('/');

  if (parts.isEmpty) {
    return null;
  }

  final scope = parts.first;
  final name = parts.skip(1).toList();

  if (scope.isEmpty || name.isEmpty || ![scope, ...name].every((part) => _identPattern.hasMatch(part))) {
    return null;
  }

  return ['secret', scope, ...name].join('.');
}

List<_Token> _tokenize(String source) {
  final tokens = <_Token>[];
  var index = 0;

  while (index < source.length) {
    final char = source[index];

    if (RegExp(r'\s').hasMatch(char)) {
      index += 1;
      continue;
    }

    if (source.startsWith('++', index) || source.startsWith('??', index)) {
      tokens.add(_Token(_TokenKind.op, source.substring(index, index + 2)));
      index += 2;
      continue;
    }

    if ('{}[]():,.'.contains(char)) {
      tokens.add(_Token(_TokenKind.punct, char));
      index += 1;
      continue;
    }

    if (char == '"') {
      final start = index;
      index += 1;
      var escaped = false;

      while (index < source.length) {
        final next = source[index];
        index += 1;

        if (escaped) {
          escaped = false;
          continue;
        }

        if (next == '\\') {
          escaped = true;
          continue;
        }

        if (next == '"') {
          break;
        }
      }

      tokens.add(_Token(_TokenKind.string, source.substring(start, index)));
      continue;
    }

    final tail = source.substring(index);
    final numberMatch = RegExp(r'^-?\d+(?:\.\d+)?').matchAsPrefix(tail);

    if (numberMatch != null) {
      tokens.add(_Token(_TokenKind.number, numberMatch.group(0)!));
      index += numberMatch.group(0)!.length;
      continue;
    }

    final identMatch = RegExp(r'^[A-Za-z_$][A-Za-z0-9_$-]*').matchAsPrefix(tail);

    if (identMatch != null) {
      tokens.add(_Token(_TokenKind.ident, identMatch.group(0)!));
      index += identMatch.group(0)!.length;
      continue;
    }

    throw FormatException('Unexpected character $char');
  }

  tokens.add(const _Token(_TokenKind.eof, ''));
  return tokens;
}

class _Parser {
  _Parser(this._tokens);

  final List<_Token> _tokens;
  int _index = 0;

  JsonValue parseExpression() => _parseCoalesce();

  _Token expect(_TokenKind kind, [String? text]) {
    final token = _peek();

    if (token.kind != kind || (text != null && token.text != text)) {
      throw FormatException(text != null ? 'Expected $text' : 'Expected $kind');
    }

    _index += 1;
    return token;
  }

  JsonValue _parseCoalesce() {
    final parts = [_parseConcat()];

    while (_match(_TokenKind.op, '??')) {
      parts.add(_parseConcat());
    }

    return parts.length == 1 ? parts.first : {r'$coalesce': parts};
  }

  JsonValue _parseConcat() {
    final parts = [_parsePrimary()];

    while (_match(_TokenKind.op, '++')) {
      parts.add(_parsePrimary());
    }

    return parts.length == 1 ? parts.first : {r'$concat': parts};
  }

  JsonValue _parsePrimary() {
    final token = _peek();

    if (_match(_TokenKind.punct, '(')) {
      final value = parseExpression();
      expect(_TokenKind.punct, ')');
      return value;
    }

    if (_match(_TokenKind.punct, '{')) {
      return _parseObject();
    }

    if (_match(_TokenKind.punct, '[')) {
      return _parseArray();
    }

    if (token.kind == _TokenKind.string) {
      _index += 1;
      return asJsonValue(jsonDecode(token.text));
    }

    if (token.kind == _TokenKind.number) {
      _index += 1;
      return token.text.contains('.') ? double.parse(token.text) : int.parse(token.text);
    }

    if (token.kind == _TokenKind.ident) {
      return _parseIdentPrimary();
    }

    throw const FormatException('Expected expression');
  }

  JsonValue _parseIdentPrimary() {
    final head = expect(_TokenKind.ident).text;

    if (head == 'true') {
      return true;
    }

    if (head == 'false') {
      return false;
    }

    if (head == 'null') {
      return null;
    }

    if ((head == 'string' || head == 'json') && _match(_TokenKind.punct, '(')) {
      final nested = parseExpression();
      expect(_TokenKind.punct, ')');
      return head == 'string' ? {r'$to_string': nested} : {r'$to_json_string': nested};
    }

    final path = [head];

    while (_match(_TokenKind.punct, '.')) {
      path.add(_expectPathSegment());
    }

    return _lowerPath(path);
  }

  JsonValue _parseObject() {
    final record = <String, Object?>{};

    while (!_match(_TokenKind.punct, '}')) {
      final keyToken = _peek();
      String key;

      if (keyToken.kind == _TokenKind.string) {
        key = jsonDecode(expect(_TokenKind.string).text) as String;
      } else {
        key = expect(_TokenKind.ident).text;
      }

      JsonValue value;

      if (_match(_TokenKind.punct, ':')) {
        value = parseExpression();
      } else {
        value = _lowerPath([key]);
      }

      record[key] = value;

      if (_match(_TokenKind.punct, ',')) {
        continue;
      }

      expect(_TokenKind.punct, '}');
      break;
    }

    return asJsonValue(record);
  }

  JsonValue _parseArray() {
    final items = <Object?>[];

    while (!_match(_TokenKind.punct, ']')) {
      items.add(parseExpression());

      if (_match(_TokenKind.punct, ',')) {
        continue;
      }

      expect(_TokenKind.punct, ']');
      break;
    }

    return items;
  }

  String _expectPathSegment() {
    final token = _peek();

    if (token.kind != _TokenKind.ident && token.kind != _TokenKind.number) {
      throw const FormatException('Expected path segment');
    }

    _index += 1;
    return token.text;
  }

  bool _match(_TokenKind kind, [String? text]) {
    final token = _peek();

    if (token.kind != kind || (text != null && token.text != text)) {
      return false;
    }

    _index += 1;
    return true;
  }

  _Token _peek() => _index < _tokens.length ? _tokens[_index] : const _Token(_TokenKind.eof, '');
}

JsonValue _lowerPath(List<String> path) {
  final head = path.first;
  final rest = path.skip(1).toList();

  if (head == 'params' || head == 'prev' || head == 'config') {
    return {
      r'$ref': {head: _pathSegments(rest)},
    };
  }

  if (head == 'run' || head == 'workflow') {
    return {
      r'$ref': {'workflow': _pathSegments(rest)},
    };
  }

  if (head == 'secret' && rest.length >= 2) {
    return 'secret://${rest[0]}/${rest.skip(1).join("/")}';
  }

  return {
    r'$ref': {'node': head, 'output': _pathSegments(rest)},
  };
}

final RegExp _digitsOnly = RegExp(r'^\d+$');

List<Object> _pathSegments(List<String> path) =>
    path.map<Object>((segment) => _digitsOnly.hasMatch(segment) ? int.parse(segment) : segment).toList();

bool _isRecord(Object? value) => value is Map<String, Object?>;
