import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../core/platform/text_editor.dart';
import '../theme/app_theme.dart';
import 'wdl_syntax.dart';

class CodeEditor extends StatefulWidget {
  const CodeEditor({
    super.key,
    required this.value,
    required this.onChanged,
    this.readOnly = false,
    this.language = TextEditorLanguage.json,
    this.minLines = 8,
    this.controller,
    this.onSelectionChanged,
  });

  final String value;
  final ValueChanged<String> onChanged;
  final bool readOnly;
  final TextEditorLanguage language;
  final int minLines;
  final TextEditingController? controller;
  final ValueChanged<TextSelection>? onSelectionChanged;

  @override
  State<CodeEditor> createState() => _CodeEditorState();
}

class _CodeEditorState extends State<CodeEditor> {
  late final TextEditingController _controller;
  late final FocusNode _focusNode;
  var _ownsController = true;

  @override
  void initState() {
    super.initState();
    _ownsController = widget.controller == null;
    _controller = widget.controller ??
        (widget.language == TextEditorLanguage.wdl
            ? WdlEditingController(text: widget.value)
            : TextEditingController(text: widget.value));
    _focusNode = FocusNode();
    _controller.addListener(_handleControllerChanged);
  }

  void _handleControllerChanged() {
    widget.onSelectionChanged?.call(_controller.selection);
  }

  @override
  void didUpdateWidget(CodeEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.controller == null && oldWidget.value != widget.value && _controller.text != widget.value) {
      _controller.text = widget.value;
    }
  }

  @override
  void dispose() {
    _controller.removeListener(_handleControllerChanged);
    if (_ownsController) _controller.dispose();
    _focusNode.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final lineCount = '\n'.allMatches(_controller.text).length + 1;
    final gutterWidth = (lineCount.toString().length * 8 + 16).toDouble();
    final codeStyle = TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 13, color: AppColors.textPrimary, height: 1.45);

    final content = Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          width: gutterWidth,
          padding: const EdgeInsets.symmetric(vertical: 10),
          color: AppColors.surfaceMuted,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              for (var i = 1; i <= lineCount; i++)
                Padding(
                  padding: const EdgeInsets.only(right: 8, bottom: 2),
                  child: Text(
                    '$i',
                    style: TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 12, color: AppColors.textMuted),
                  ),
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
            style: codeStyle,
            cursorColor: AppColors.accent,
            decoration: const InputDecoration(
              border: InputBorder.none,
              contentPadding: EdgeInsets.all(10),
              isDense: true,
              filled: false,
            ),
            onChanged: widget.onChanged,
          ),
        ),
      ],
    );

    return Container(
      decoration: BoxDecoration(
        color: AppColors.surfaceSubtle,
        borderRadius: BorderRadius.circular(AppMetrics.radiusSm),
        border: Border.all(color: AppColors.border),
      ),
      // when the host gives us a bounded height (an Expanded pane, a fixed SizedBox), scroll
      // internally instead of growing the gutter/text field past that bound.
      child: LayoutBuilder(
        builder: (context, constraints) {
          if (constraints.hasBoundedHeight) {
            return SingleChildScrollView(child: content);
          }
          return content;
        },
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
  const WdlEditor({
    super.key,
    required this.value,
    required this.onChanged,
    this.readOnly = false,
    this.onComplete,
    this.onHover,
  });

  final String value;
  final ValueChanged<String> onChanged;
  final bool readOnly;
  final Future<List<WdlCompletionSuggestion>> Function(int cursorOffset, String source)? onComplete;
  final Future<WdlHoverInfo?> Function(int cursorOffset, String source)? onHover;

  @override
  Widget build(BuildContext context) {
    if (onComplete == null && onHover == null) {
      return CodeEditor(value: value, onChanged: onChanged, readOnly: readOnly, language: TextEditorLanguage.wdl, minLines: 16);
    }
    return WdlSmartEditor(
      value: value,
      onChanged: onChanged,
      readOnly: readOnly,
      onComplete: onComplete!,
      onHover: onHover,
    );
  }
}

class WdlCompletionSuggestion {
  const WdlCompletionSuggestion({required this.label, required this.insertText, this.detail});

  final String label;
  final String insertText;
  final String? detail;
}

class WdlHoverInfo {
  const WdlHoverInfo({required this.title, this.documentation});

  final String title;
  final String? documentation;
}

class WdlSmartEditor extends StatefulWidget {
  const WdlSmartEditor({
    super.key,
    required this.value,
    required this.onChanged,
    required this.onComplete,
    this.readOnly = false,
    this.onHover,
  });

  final String value;
  final ValueChanged<String> onChanged;
  final bool readOnly;
  final Future<List<WdlCompletionSuggestion>> Function(int cursorOffset, String source) onComplete;
  final Future<WdlHoverInfo?> Function(int cursorOffset, String source)? onHover;

  @override
  State<WdlSmartEditor> createState() => _WdlSmartEditorState();
}

class _WdlSmartEditorState extends State<WdlSmartEditor> {
  late final TextEditingController _controller;
  List<WdlCompletionSuggestion> _suggestions = const [];
  WdlHoverInfo? _hover;

  @override
  void initState() {
    super.initState();
    _controller = WdlEditingController(text: widget.value);
  }

  @override
  void didUpdateWidget(WdlSmartEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.value != widget.value && _controller.text != widget.value) {
      _controller.text = widget.value;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _updateHover() async {
    if (widget.onHover == null) return;
    final hover = await widget.onHover!(_controller.selection.baseOffset, _controller.text);
    if (mounted) setState(() => _hover = hover);
  }

  Future<void> _triggerCompletion() async {
    final items = await widget.onComplete(_controller.selection.baseOffset, _controller.text);
    if (mounted) setState(() => _suggestions = items);
  }

  void _applySuggestion(WdlCompletionSuggestion item) {
    final text = _controller.text;
    final cursor = _controller.selection.baseOffset;
    final next = '${text.substring(0, cursor)}${item.insertText}${text.substring(cursor)}';
    _controller.value = TextEditingValue(text: next, selection: TextSelection.collapsed(offset: cursor + item.insertText.length));
    widget.onChanged(next);
    setState(() => _suggestions = const []);
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          child: CodeEditor(
            controller: _controller,
            value: _controller.text,
            readOnly: widget.readOnly,
            onChanged: widget.onChanged,
            language: TextEditorLanguage.wdl,
            minLines: 16,
            onSelectionChanged: (_) => _updateHover(),
          ),
        ),
        if (_hover != null)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text('${_hover!.title}${_hover!.documentation != null ? ' — ${_hover!.documentation}' : ''}', style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
          ),
        if (_suggestions.isNotEmpty)
          Container(
            constraints: const BoxConstraints(maxHeight: 160),
            margin: const EdgeInsets.only(top: 6),
            decoration: BoxDecoration(border: Border.all(color: AppColors.border), borderRadius: BorderRadius.circular(6)),
            child: ListView.builder(
              itemCount: _suggestions.length,
              itemBuilder: (context, index) {
                final item = _suggestions[index];
                return ListTile(
                  dense: true,
                  title: Text(item.label, style: const TextStyle(fontSize: 12)),
                  subtitle: item.detail == null ? null : Text(item.detail!, style: const TextStyle(fontSize: 10)),
                  onTap: () => _applySuggestion(item),
                );
              },
            ),
          ),
        Align(
          alignment: Alignment.centerRight,
          child: TextButton(onPressed: widget.readOnly ? null : _triggerCompletion, child: const Text('Complete')),
        ),
      ],
    );
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
