import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/json.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/secrets_service.dart';
import '../../core/utils/format.dart';
import '../../core/utils/json_utils.dart';
import '../../core/utils/secrets.dart';
import '../../core/utils/workflow_references.dart';
import 'code_editor.dart';
import 'typed_value_editor.dart';

class TypedParameterEditor extends ConsumerWidget {
  const TypedParameterEditor({
    super.key,
    required this.parameters,
    required this.value,
    required this.onChanged,
    this.credentialScopes = const [],
    this.expressionContext,
  });

  final List<ActionParameterMetadata> parameters;
  final JsonRecord value;
  final ValueChanged<JsonRecord> onChanged;
  final List<String> credentialScopes;
  final WorkflowExpressionEditorContext? expressionContext;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (parameters.isEmpty) {
      return const Text('This action does not publish typed parameters yet.', style: TextStyle(fontSize: 12));
    }

    final secrets = ref.watch(secretsProvider.notifier).secretsForScopes(credentialScopes);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final parameter in parameters) ...[
          Text('${parameter.label ?? parameter.name}${parameter.required ? ' *' : ''}', style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 12)),
          if (parameter.description != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 4),
              child: Text(parameter.description!, style: const TextStyle(fontSize: 11, color: Colors.grey)),
            ),
          if (parameter.secret)
            DropdownButtonFormField<String>(
              isExpanded: true,
              value: _secretValue(parameter.name),
              items: [
                if (_secretValue(parameter.name).isNotEmpty && !secrets.any((s) => secretRef(s.scope, s.name) == _secretValue(parameter.name)))
                  DropdownMenuItem(value: _secretValue(parameter.name), child: Text(secretRefLabel(_secretValue(parameter.name)))),
                for (final secret in secrets)
                  DropdownMenuItem(value: secretRef(secret.scope, secret.name), child: Text('${secret.scope}/${secret.name}')),
              ],
              onChanged: (next) => _setValue(parameter.name, next ?? ''),
            )
          else
            TypedValueEditor(
              value: value[parameter.name],
              ty: parameter.ty,
              placeholder: parameter.label ?? parameter.name,
              expressionContext: expressionContext,
              onChanged: (next) => _setValue(parameter.name, next),
            ),
          const SizedBox(height: 12),
        ],
      ],
    );
  }

  String _secretValue(String name) => value[name]?.toString() ?? '';

  void _setValue(String name, JsonValue next) {
    onChanged({...value, name: next});
  }
}

class KeyValueObjectEditor extends StatefulWidget {
  const KeyValueObjectEditor({
    super.key,
    required this.title,
    required this.value,
    required this.onChanged,
    this.emptyLabel = 'No entries.',
    this.expressionContext,
  });

  final String title;
  final JsonRecord value;
  final ValueChanged<JsonRecord> onChanged;
  final String emptyLabel;
  final WorkflowExpressionEditorContext? expressionContext;

  @override
  State<KeyValueObjectEditor> createState() => _KeyValueObjectEditorState();
}

class _KeyValueObjectEditorState extends State<KeyValueObjectEditor> {
  @override
  Widget build(BuildContext context) {
    final entries = widget.value.entries.toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(child: Text(widget.title, style: const TextStyle(fontWeight: FontWeight.w600))),
            TextButton(onPressed: () => widget.onChanged({...widget.value, 'key_${entries.length + 1}': ''}), child: const Text('Add')),
          ],
        ),
        if (entries.isEmpty) Text(widget.emptyLabel, style: const TextStyle(fontSize: 12, color: Colors.grey)),
        for (final entry in entries)
          Row(
            children: [
              Expanded(
                flex: 2,
                child: TextField(
                  decoration: const InputDecoration(labelText: 'Key', isDense: true),
                  controller: TextEditingController(text: entry.key),
                  onChanged: (key) {
                    final next = Map<String, Object?>.from(widget.value)..remove(entry.key);
                    if (key.isNotEmpty) next[key] = entry.value;
                    widget.onChanged(next);
                  },
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                flex: 3,
                child: TypedValueEditor(
                  value: entry.value,
                  ty: const RuninatorTypeAny(),
                  placeholder: 'Value',
                  expressionContext: widget.expressionContext,
                  onChanged: (v) => widget.onChanged({...widget.value, entry.key: v}),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.delete_outline, size: 16),
                onPressed: () {
                  final next = Map<String, Object?>.from(widget.value)..remove(entry.key);
                  widget.onChanged(next);
                },
              ),
            ],
          ),
      ],
    );
  }
}

class AdvancedWdlParameters extends StatefulWidget {
  const AdvancedWdlParameters({super.key, required this.value, required this.onChanged, this.title = 'Raw WDL parameters'});

  final String value;
  final ValueChanged<String> onChanged;
  final String title;

  @override
  State<AdvancedWdlParameters> createState() => _AdvancedWdlParametersState();
}

class _AdvancedWdlParametersState extends State<AdvancedWdlParameters> {
  var _expanded = false;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InkWell(
          onTap: () => setState(() => _expanded = !_expanded),
          child: Row(
            children: [
              Icon(_expanded ? Icons.expand_more : Icons.chevron_right, size: 16),
              Text(widget.title, style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 12)),
            ],
          ),
        ),
        if (_expanded)
          SizedBox(
            height: 140,
            child: JsonEditor(value: widget.value, onChanged: widget.onChanged),
          ),
      ],
    );
  }
}

JsonRecord parseStepParameters(String json) => parseObject(json, {});

void writeStepParameters(JsonRecord value, void Function(String json) onChanged) {
  onChanged(const JsonEncoder.withIndent('  ').convert(value));
}
