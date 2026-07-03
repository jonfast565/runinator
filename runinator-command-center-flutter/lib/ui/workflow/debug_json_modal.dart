import 'dart:convert';

import 'package:flutter/material.dart';

import '../../core/utils/format.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';

class DebugJsonModal extends StatefulWidget {
  const DebugJsonModal({
    super.key,
    required this.title,
    required this.initialValue,
    required this.onSubmit,
    required this.onClose,
    this.hint,
    this.submitLabel = 'Submit',
  });

  final String title;
  final Object? initialValue;
  final ValueChanged<Object?> onSubmit;
  final VoidCallback onClose;
  final String? hint;
  final String submitLabel;

  @override
  State<DebugJsonModal> createState() => _DebugJsonModalState();
}

class _DebugJsonModalState extends State<DebugJsonModal> {
  late String _text;
  String? _error;

  @override
  void initState() {
    super.initState();
    _text = pretty(widget.initialValue ?? {});
  }

  bool get _isValid {
    try {
      jsonDecode(_text);
      return true;
    } catch (_) {
      return false;
    }
  }

  void _submit() {
    try {
      widget.onSubmit(jsonDecode(_text));
    } catch (err) {
      setState(() => _error = err.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.black54,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 640, maxHeight: 560),
          child: Card(
            margin: const EdgeInsets.all(24),
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Expanded(child: Text(widget.title, style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 16))),
                      IconButton(icon: const Icon(Icons.close), onPressed: widget.onClose),
                    ],
                  ),
                  if (widget.hint != null) Text(widget.hint!, style: const TextStyle(fontSize: 12, color: Colors.grey)),
                  const SizedBox(height: 8),
                  Expanded(child: JsonEditor(value: _text, onChanged: (v) => setState(() { _text = v; _error = null; }))),
                  if (_error != null) Text(_error!, style: const TextStyle(color: Colors.red, fontSize: 12)),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    children: [
                      CcButton(label: 'Cancel', onPressed: widget.onClose),
                      CcButton(label: widget.submitLabel, variant: CcButtonVariant.primary, onPressed: _isValid ? _submit : null),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
