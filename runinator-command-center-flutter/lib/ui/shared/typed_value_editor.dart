import 'package:flutter/material.dart';

import '../../core/domain/json.dart';
import '../../core/domain/models/index.dart';
import '../../core/utils/wdl_expression.dart';
import 'code_editor.dart';

class TypedValueEditor extends StatefulWidget {
  const TypedValueEditor({
    super.key,
    required this.value,
    required this.ty,
    required this.onChanged,
    this.placeholder,
    this.allowExpressions = true,
  });

  final JsonValue value;
  final RuninatorType ty;
  final ValueChanged<JsonValue> onChanged;
  final String? placeholder;
  final bool allowExpressions;

  @override
  State<TypedValueEditor> createState() => _TypedValueEditorState();
}

class _TypedValueEditorState extends State<TypedValueEditor> {
  var _expressionMode = false;

  @override
  void initState() {
    super.initState();
    _expressionMode = isWorkflowExpressionValue(widget.value);
  }

  String get _typeKind {
    if (widget.ty is RuninatorTypeString) return 'string';
    if (widget.ty is RuninatorTypeNumber) return 'number';
    if (widget.ty is RuninatorTypeInteger) return 'integer';
    if (widget.ty is RuninatorTypeBoolean) return 'boolean';
    if (widget.ty is RuninatorTypeArray) return 'array';
    if (widget.ty is RuninatorTypeStruct) return 'struct';
    if (widget.ty is RuninatorTypeMap) return 'map';
    if (widget.ty is RuninatorTypeEnum) return 'enum';
    return 'any';
  }

  @override
  Widget build(BuildContext context) {
    if (widget.allowExpressions) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SegmentedButton<bool>(
            segments: const [
              ButtonSegment(value: false, label: Text('Value')),
              ButtonSegment(value: true, label: Text('Expression')),
            ],
            selected: {_expressionMode},
            onSelectionChanged: (selection) {
              setState(() => _expressionMode = selection.first);
            },
          ),
          const SizedBox(height: 8),
          if (_expressionMode)
            JsonEditor(
              value: widget.value?.toString() ?? '{}',
              onChanged: (text) {
                try {
                  widget.onChanged(parseJsonValue(text));
                } catch (_) {}
              },
            )
          else
            _valueEditor(),
        ],
      );
    }

    return _valueEditor();
  }

  Widget _valueEditor() {
    switch (_typeKind) {
      case 'string':
        return TextField(
          decoration: InputDecoration(hintText: widget.placeholder ?? 'string'),
          controller: TextEditingController(text: widget.value?.toString() ?? ''),
          onChanged: (v) => widget.onChanged(v),
        );
      case 'number':
      case 'integer':
        return TextField(
          decoration: InputDecoration(hintText: widget.placeholder ?? _typeKind),
          keyboardType: TextInputType.number,
          controller: TextEditingController(text: widget.value?.toString() ?? ''),
          onChanged: (v) {
            final parsed = _typeKind == 'integer' ? int.tryParse(v) : num.tryParse(v);
            widget.onChanged(parsed ?? v);
          },
        );
      case 'boolean':
        return CheckboxListTile(
          contentPadding: EdgeInsets.zero,
          title: const Text('true'),
          value: widget.value == true,
          onChanged: (v) => widget.onChanged(v ?? false),
        );
      case 'struct':
        final struct = widget.ty as RuninatorTypeStruct;
        return Column(
          children: [
            for (final entry in struct.fields.entries)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('${entry.key}${entry.value.required ? ' *' : ''}', style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 12)),
                    TypedValueEditor(
                      value: (widget.value is Map ? (widget.value as Map)[entry.key] : null) as JsonValue?,
                      ty: entry.value.ty,
                      onChanged: (next) {
                        final map = widget.value is Map ? Map<String, Object?>.from(widget.value as Map) : <String, Object?>{};
                        map[entry.key] = next;
                        widget.onChanged(map);
                      },
                    ),
                  ],
                ),
              ),
          ],
        );
      default:
        return JsonEditor(
          value: encodeJsonPretty(widget.value),
          onChanged: (text) {
            try {
              widget.onChanged(parseJsonValue(text));
            } catch (_) {}
          },
        );
    }
  }

  String encodeJsonPretty(JsonValue? value) {
    if (value == null) return 'null';
    if (value is String) return '"$value"';
    if (value is Map || value is List) return value.toString();
    return value.toString();
  }

  JsonValue parseJsonValue(String text) {
    final trimmed = text.trim();
    if (trimmed == 'null') return null;
    if (trimmed == 'true') return true;
    if (trimmed == 'false') return false;
    if (trimmed.startsWith('"') && trimmed.endsWith('"')) return trimmed.substring(1, trimmed.length - 1);
    final numVal = num.tryParse(trimmed);
    if (numVal != null) return numVal;
    return trimmed;
  }
}

String encodeJsonPretty(JsonValue? value) {
  if (value == null) return 'null';
  if (value is String) return '"$value"';
  return value.toString();
}
