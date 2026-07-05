import 'package:flutter/material.dart';

// lightweight single-pass wdl tokenizer for syntax highlighting inside a plain flutter
// TextField (via TextEditingController.buildTextSpan). mirrors the keyword vocabulary of
// runinator-command-center's codemirror-lang-wdl.ts so both surfaces color the same words,
// though this scans in one linear pass rather than tracking codemirror's full parser state
// (no type-body/binding-name context) — good enough to read the language, not to parse it.
// the color scheme is a dark-theme adaptation of that file's "one light inspired" palette,
// since this editor renders on a dark background.

// structural declarations that open blocks or bind names.
const Set<String> _declKw = {
  'workflow', 'params', 'input', 'node', 'let', 'type', 'alias', 'trigger',
  'start', 'set', 'secret', 'config', 'fn', 'namespace', 'import',
};

// control-flow statements and block headers.
const Set<String> _controlKw = {
  'if', 'else', 'for', 'while', 'until', 'match', 'when', 'toggle', 'split',
  'on', 'off', 'parallel', 'race', 'try', 'catch', 'finally', 'map', 'branch',
  'join', 'wait', 'emit', 'output', 'yield', 'approve', 'fail', 'subflow',
  'compute', 'return', 'goto', 'edges', 'gate', 'signal', 'watch',
  'compensate', 'assert', 'transform', 'audit', 'checkpoint', 'mutex',
  'throttle', 'await', 'debounce', 'collect', 'barrier', 'circuit_breaker',
  'event_source',
};

// clause/option words that modify a statement.
const Set<String> _modifierKw = {
  'with', 'as', 'initial', 'limit', 'concurrency', 'detached', 'reuse',
  'disabled', 'blackout', 'to', 'cron', 'winner', 'name', 'meta', 'returns',
  'every', 'timeout', 'key', 'priority', 'max_depth', 'rate', 'per', 'delay',
  'count', 'threshold', 'window', 'cooldown', 'mode', 'action', 'actor',
  'target', 'reason', 'filter',
};

// word-form comparison/membership operators.
const Set<String> _opKw = {'exists', 'contains', 'in', 'starts_with', 'ends_with'};

// outcome labels, only highlighted as such when they precede a `->` transition.
const Set<String> _outcomes = {'ok', 'next', 'fail', 'timeout', 'reject'};

// constant-like policy/target atoms.
const Set<String> _atoms = {
  'all', 'any', 'first_success', 'done', 'none', 'manual', 'condition', 'external',
};

// coercion and compile-time intrinsics, highlighted as functions only when called.
const Set<String> _builtins = {'string', 'json', 'file', 'dir', 'inline'};

// reference roots that are never keywords.
const Set<String> _pureRefs = {'run', 'loop', 'state', 'item'};

// roots that double as keywords; treated as a reference only before a `.`.
const Set<String> _rootKeywords = {'params', 'config', 'secret', 'workflow', 'std'};

// primitive type names.
const Set<String> _types = {
  'any', 'boolean', 'bool', 'duration', 'float', 'int', 'integer', 'json',
  'map', 'null', 'number', 'string',
};

// one-dark-inspired palette, chosen to read on the editor's #0F1720 background while
// preserving the same semantic hue groupings as the vue app's light scheme (purple
// keywords, green strings, orange numbers/atoms, cyan types, red refs, blue functions).
class _WdlColors {
  static const comment = Color(0xFF7C8494);
  static const keyword = Color(0xFFC678DD);
  static const opKeyword = Color(0xFF56B6C2);
  static const outcome = Color(0xFFE5C07B);
  static const atom = Color(0xFFD19A66);
  static const boolNull = Color(0xFF56B6C2);
  static const typeName = Color(0xFF61AFEF);
  static const provider = Color(0xFFE5C07B);
  static const function = Color(0xFF61AFEF);
  static const refRoot = Color(0xFFE06C75);
  static const property = Color(0xFFDCDFE4);
  static const annotation = Color(0xFFE5C07B);
  static const number = Color(0xFFD19A66);
  static const string = Color(0xFF98C379);
  static const arrow = Color(0xFFC678DD);
  static const operatorColor = Color(0xFFABB2BF);
  static const bracket = Color(0xFFABB2BF);
  static const defaultText = Color(0xFFE5E7EB);
}

bool _isIdentStart(int code) =>
    (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || code == 95;

bool _isIdentPart(int code) => _isIdentStart(code) || (code >= 48 && code <= 57);

bool _isDigit(int code) => code >= 48 && code <= 57;

bool _isSpace(int code) => code == 32 || code == 9 || code == 10 || code == 13;

/// builds highlighted [TextSpan] children covering the whole [text], for use from a
/// [TextEditingController.buildTextSpan] override. [base] supplies the font family/size;
/// only [TextStyle.color]/[FontStyle]/[FontWeight] vary per token.
List<TextSpan> buildWdlSpans(String text, TextStyle? base) {
  final spans = <TextSpan>[];
  var i = 0;
  final len = text.length;
  var afterDot = false;

  void emit(int start, int end, {Color? color, FontStyle? fontStyle, FontWeight? fontWeight}) {
    if (end <= start) return;
    spans.add(TextSpan(
      text: text.substring(start, end),
      style: color == null ? null : base?.copyWith(color: color, fontStyle: fontStyle, fontWeight: fontWeight),
    ));
  }

  // true when the next non-space characters (without consuming) match `.` then an identifier
  // then `(` — i.e. this identifier is a provider name in `provider.action(...)`.
  bool peekIsProviderCall(int from) {
    var j = from;
    while (j < len && _isSpace(text.codeUnitAt(j))) {
      j++;
    }
    if (j >= len || text[j] != '.') return false;
    j++;
    while (j < len && _isSpace(text.codeUnitAt(j))) {
      j++;
    }
    final identStart = j;
    while (j < len && _isIdentPart(text.codeUnitAt(j))) {
      j++;
    }
    if (j == identStart) return false;
    while (j < len && _isSpace(text.codeUnitAt(j))) {
      j++;
    }
    return j < len && text[j] == '(';
  }

  bool peekIsFollowedByOpenParen(int from) {
    var j = from;
    while (j < len && _isSpace(text.codeUnitAt(j))) {
      j++;
    }
    return j < len && text[j] == '(';
  }

  bool peekIsFollowedByArrow(int from) {
    var j = from;
    while (j < len && _isSpace(text.codeUnitAt(j))) {
      j++;
    }
    return j + 1 < len && text[j] == '-' && text[j + 1] == '>';
  }

  bool peekIsFollowedByDot(int from) {
    var j = from;
    while (j < len && _isSpace(text.codeUnitAt(j))) {
      j++;
    }
    return j < len && text[j] == '.';
  }

  while (i < len) {
    final start = i;
    final ch = text[i];
    final code = text.codeUnitAt(i);
    final wasAfterDot = afterDot;
    afterDot = false;

    if (_isSpace(code)) {
      i++;
      while (i < len && _isSpace(text.codeUnitAt(i))) {
        i++;
      }
      afterDot = wasAfterDot;
      emit(start, i);
      continue;
    }

    // line comment.
    if (ch == '/' && i + 1 < len && text[i + 1] == '/') {
      final j = text.indexOf('\n', i);
      final end = j == -1 ? len : j;
      emit(start, end, color: _WdlColors.comment, fontStyle: FontStyle.italic);
      i = end;
      continue;
    }

    // block comment.
    if (ch == '/' && i + 1 < len && text[i + 1] == '*') {
      final j = text.indexOf('*/', i + 2);
      final end = j == -1 ? len : j + 2;
      emit(start, end, color: _WdlColors.comment, fontStyle: FontStyle.italic);
      i = end;
      continue;
    }

    // string literal.
    if (ch == '"') {
      var j = i + 1;
      var escaped = false;
      while (j < len) {
        final c = text[j];
        if (escaped) {
          escaped = false;
        } else if (c == '\\') {
          escaped = true;
        } else if (c == '"') {
          j++;
          break;
        } else if (c == '\n') {
          break;
        }
        j++;
      }
      emit(start, j, color: _WdlColors.string);
      i = j;
      continue;
    }

    // annotation @id.
    if (ch == '@' && i + 1 < len && _isIdentStart(text.codeUnitAt(i + 1))) {
      var j = i + 1;
      while (j < len && _isIdentPart(text.codeUnitAt(j))) {
        j++;
      }
      emit(start, j, color: _WdlColors.annotation);
      i = j;
      continue;
    }

    // number, with optional duration suffix (s/m/h/d).
    if (_isDigit(code) || (ch == '-' && i + 1 < len && _isDigit(text.codeUnitAt(i + 1)))) {
      var j = i + (ch == '-' ? 1 : 0);
      final numStart = j;
      while (j < len && _isDigit(text.codeUnitAt(j))) {
        j++;
      }
      if (j < len && text[j] == '.' && j + 1 < len && _isDigit(text.codeUnitAt(j + 1))) {
        j++;
        while (j < len && _isDigit(text.codeUnitAt(j))) {
          j++;
        }
      }
      if (j > numStart) {
        if (j < len && 'smhd'.contains(text[j]) && (j + 1 >= len || !_isIdentPart(text.codeUnitAt(j + 1)))) {
          j++;
        }
        emit(start, j, color: _WdlColors.number);
        i = j;
        continue;
      }
    }

    // arrow.
    if (ch == '-' && i + 1 < len && text[i + 1] == '>') {
      emit(start, i + 2, color: _WdlColors.arrow, fontWeight: FontWeight.w600);
      i += 2;
      continue;
    }

    // identifiers, keywords, references.
    if (_isIdentStart(code)) {
      var j = i + 1;
      while (j < len && _isIdentPart(text.codeUnitAt(j))) {
        j++;
      }
      final word = text.substring(i, j);

      if (wasAfterDot) {
        emit(start, j, color: peekIsFollowedByOpenParen(j) ? _WdlColors.function : _WdlColors.property);
        i = j;
        continue;
      }

      if (peekIsProviderCall(j)) {
        emit(start, j, color: _WdlColors.provider);
        i = j;
        continue;
      }

      if (_pureRefs.contains(word) || (_rootKeywords.contains(word) && peekIsFollowedByDot(j))) {
        emit(start, j, color: _WdlColors.refRoot, fontStyle: FontStyle.italic);
        i = j;
        continue;
      }

      if (_outcomes.contains(word) && peekIsFollowedByArrow(j)) {
        emit(start, j, color: _WdlColors.outcome, fontWeight: FontWeight.w600);
        i = j;
        continue;
      }

      if (_builtins.contains(word) && peekIsFollowedByOpenParen(j)) {
        emit(start, j, color: _WdlColors.function);
        i = j;
        continue;
      }

      if (word == 'true' || word == 'false' || word == 'null') {
        emit(start, j, color: _WdlColors.boolNull);
        i = j;
        continue;
      }

      if (_atoms.contains(word)) {
        emit(start, j, color: _WdlColors.atom);
        i = j;
        continue;
      }

      if (_types.contains(word)) {
        emit(start, j, color: _WdlColors.typeName);
        i = j;
        continue;
      }

      if (_declKw.contains(word) || _controlKw.contains(word) || _modifierKw.contains(word)) {
        emit(start, j, color: _WdlColors.keyword);
        i = j;
        continue;
      }

      if (_opKw.contains(word)) {
        emit(start, j, color: _WdlColors.opKeyword);
        i = j;
        continue;
      }

      emit(start, j, color: _WdlColors.defaultText);
      i = j;
      continue;
    }

    // dot: routes the next identifier to property/method coloring.
    if (ch == '.') {
      afterDot = true;
      emit(start, i + 1, color: _WdlColors.operatorColor);
      i++;
      continue;
    }

    // braces/brackets/parens.
    if ('(){}[]'.contains(ch)) {
      emit(start, i + 1, color: _WdlColors.bracket);
      i++;
      continue;
    }

    // multi-char operators.
    const multiCharOps = ['++', '??', '&&', '||', '!=', '==', '>=', '<=', '=>', '...'];
    final matchedOp = multiCharOps.firstWhere(
      (op) => text.startsWith(op, i),
      orElse: () => '',
    );
    if (matchedOp.isNotEmpty) {
      emit(start, i + matchedOp.length, color: _WdlColors.operatorColor);
      i += matchedOp.length;
      continue;
    }

    // remaining single-char operators/punctuation.
    if ('<>!+?*/%|&=:,;-'.contains(ch)) {
      emit(start, i + 1, color: _WdlColors.operatorColor);
      i++;
      continue;
    }

    // fallback: one unstyled character.
    emit(start, i + 1);
    i++;
  }

  return spans;
}

/// text editing controller that renders wdl syntax highlighting via [buildWdlSpans].
/// standard flutter technique for lightweight highlighting inside a plain `TextField`
/// (no external editor widget) — cursor, selection, and IME composition all keep working
/// since only the rendered [TextSpan] colors change, not the underlying editing behavior.
class WdlEditingController extends TextEditingController {
  WdlEditingController({super.text});

  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    return TextSpan(style: style, children: buildWdlSpans(text, style));
  }
}
