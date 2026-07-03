import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/json.dart';
import '../../core/domain/models/index.dart';
import '../../core/utils/format.dart';
import '../../core/utils/inputs.dart';
import 'code_editor.dart';
import 'typed_value_editor.dart';

class RunInputForm extends ConsumerStatefulWidget {
  const RunInputForm({
    super.key,
    required this.inputType,
    required this.draft,
    required this.onChanged,
    this.storageKey,
  });

  final RuninatorType? inputType;
  final JsonRecord draft;
  final ValueChanged<JsonRecord> onChanged;
  final String? storageKey;

  @override
  ConsumerState<RunInputForm> createState() => _RunInputFormState();
}

class _RunInputFormState extends ConsumerState<RunInputForm> {
  var _jsonMode = false;
  late TextEditingController _jsonController;

  @override
  void initState() {
    super.initState();
    _jsonController = TextEditingController(text: pretty(widget.draft));
  }

  @override
  void dispose() {
    _jsonController.dispose();
    super.dispose();
  }

  RuninatorTypeStruct? get _struct =>
      widget.inputType is RuninatorTypeStruct ? widget.inputType as RuninatorTypeStruct : null;

  @override
  Widget build(BuildContext context) {
    final struct = _struct;
    if (struct == null || struct.fields.isEmpty) {
      return const Text('This workflow has no input parameters.', style: TextStyle(fontSize: 12));
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SegmentedButton<bool>(
          segments: const [
            ButtonSegment(value: false, label: Text('Form')),
            ButtonSegment(value: true, label: Text('JSON')),
          ],
          selected: {_jsonMode},
          onSelectionChanged: (s) => setState(() => _jsonMode = s.first),
        ),
        const SizedBox(height: 12),
        if (_jsonMode)
          JsonEditor(
            value: _jsonController.text,
            onChanged: (text) {
              _jsonController.text = text;
              try {
                final parsed = jsonDecode(text) as Map<String, Object?>;
                widget.onChanged(parsed);
              } catch (_) {}
            },
          )
        else
          ...[
            for (final entry in struct.fields.entries)
              Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('${entry.key}${entry.value.required ? ' *' : ''}', style: const TextStyle(fontWeight: FontWeight.w600)),
                    TypedValueEditor(
                      value: widget.draft[entry.key],
                      ty: entry.value.ty,
                      allowExpressions: false,
                      onChanged: (value) {
                        final next = Map<String, Object?>.from(widget.draft);
                        next[entry.key] = value;
                        widget.onChanged(next);
                        _jsonController.text = pretty(next);
                      },
                    ),
                  ],
                ),
              ),
          ],
      ],
    );
  }
}
