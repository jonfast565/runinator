import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/platform/text_editor.dart';
import '../theme/app_theme.dart';

class CodeEditor extends StatefulWidget {
  const CodeEditor({
    super.key,
    required this.value,
    required this.onChanged,
    this.readOnly = false,
    this.language = TextEditorLanguage.json,
    this.minLines = 8,
  });

  final String value;
  final ValueChanged<String> onChanged;
  final bool readOnly;
  final TextEditorLanguage language;
  final int minLines;

  @override
  State<CodeEditor> createState() => _CodeEditorState();
}

class _CodeEditorState extends State<CodeEditor> {
  late final TextEditingController _controller;
  late final FocusNode _focusNode;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.value);
    _focusNode = FocusNode();
  }

  @override
  void didUpdateWidget(CodeEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.value != widget.value && _controller.text != widget.value) {
      _controller.text = widget.value;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final lineCount = '\n'.allMatches(_controller.text).length + 1;
    final gutterWidth = (lineCount.toString().length * 8 + 16).toDouble();

    return Container(
      decoration: BoxDecoration(
        color: const Color(0xFF0F1720),
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: AppColors.border),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: gutterWidth,
            padding: const EdgeInsets.symmetric(vertical: 10),
            color: const Color(0xFF111827),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                for (var i = 1; i <= lineCount; i++)
                  Padding(
                    padding: const EdgeInsets.only(right: 8, bottom: 2),
                    child: Text('$i', style: const TextStyle(fontFamily: 'monospace', fontSize: 12, color: Color(0xFF6B7280))),
                  ),
              ],
            ),
          ),
          Expanded(
            child: TextField(
              controller: _controller,
              focusNode: _focusNode,
              readOnly: widget.readOnly,
              maxLines: null,
              minLines: widget.minLines,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 13, color: Color(0xFFE5E7EB), height: 1.45),
              decoration: const InputDecoration(
                border: InputBorder.none,
                contentPadding: EdgeInsets.all(10),
                isDense: true,
              ),
              onChanged: widget.onChanged,
            ),
          ),
        ],
      ),
    );
  }
}

class JsonEditor extends StatelessWidget {
  const JsonEditor({super.key, required this.value, required this.onChanged, this.readOnly = false});

  final String value;
  final ValueChanged<String> onChanged;
  final bool readOnly;

  @override
  Widget build(BuildContext context) {
    return CodeEditor(value: value, onChanged: onChanged, readOnly: readOnly, language: TextEditorLanguage.json);
  }
}

class WdlEditor extends StatelessWidget {
  const WdlEditor({super.key, required this.value, required this.onChanged, this.readOnly = false});

  final String value;
  final ValueChanged<String> onChanged;
  final bool readOnly;

  @override
  Widget build(BuildContext context) {
    return CodeEditor(value: value, onChanged: onChanged, readOnly: readOnly, language: TextEditorLanguage.wdl, minLines: 16);
  }
}

class JsonView extends StatelessWidget {
  const JsonView({super.key, required this.data});

  final Object? data;

  @override
  Widget build(BuildContext context) {
    final text = data == null ? '{}' : data.toString();
    return JsonEditor(value: text, onChanged: (_) {}, readOnly: true);
  }
}

class FlutterTextEditorHost implements TextEditorHost {
  FlutterTextEditorHost(this._options);

  final TextEditorHostCreateOptions _options;
  String _value = '';

  @override
  void destroy() {}

  @override
  List<TextEditorDiagnostic> getDiagnostics() => const [];

  @override
  String getValue() => _value;

  @override
  void goToPosition(int line, [int? column]) {}

  @override
  void mount(Object container) {
    _value = _options.value;
  }

  @override
  void setReadonly(bool readonly) {}

  @override
  void setValue(String value, {bool silent = false}) {
    _value = value;
    if (!silent) {
      _options.onChange(value);
    }
  }

  @override
  void focus() {}

  @override
  Future<void> formatDocument() async {}
}

class FlutterTextEditorHostFactory implements TextEditorHostFactory {
  @override
  TextEditorHost create(TextEditorHostCreateOptions options) => FlutterTextEditorHost(options);
}
