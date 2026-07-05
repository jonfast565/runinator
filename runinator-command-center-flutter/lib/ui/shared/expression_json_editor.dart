import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/services/expression_service.dart';
import '../../core/utils/format.dart';
import '../../core/utils/wdl_expression.dart';
import '../../core/utils/workflow_references.dart';
import '../../core/platform/text_editor.dart';
import '../theme/app_theme.dart';
import 'code_editor.dart';
import 'reference_picker.dart';

class ExpressionJsonEditor extends ConsumerStatefulWidget {
  const ExpressionJsonEditor({
    super.key,
    required this.value,
    required this.onChanged,
    this.title = 'WDL Expression',
    this.readOnly = false,
    this.context,
  });

  /// lowered json expression string.
  final String value;
  final ValueChanged<String> onChanged;
  final String title;
  final bool readOnly;
  final WorkflowExpressionEditorContext? context;

  @override
  ConsumerState<ExpressionJsonEditor> createState() => _ExpressionJsonEditorState();
}

class _ExpressionJsonEditorState extends ConsumerState<ExpressionJsonEditor> {
  late final TextEditingController _controller;
  var _showPicker = false;
  var _parseError = '';
  var _previewResult = '';
  var _previewError = '';
  var _previewUnresolved = '';
  Timer? _previewTimer;
  var _previewToken = 0;
  String? _lastEmitted;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: _wdlFromLowered(widget.value));
    _controller.addListener(_handleChanged);
    _schedulePreview();
  }

  @override
  void didUpdateWidget(ExpressionJsonEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.value != oldWidget.value && widget.value != _lastEmitted) {
      final wdl = _wdlFromLowered(widget.value);
      if (_controller.text != wdl) {
        _controller.text = wdl;
      }
    }
    if (widget.context?.sampleContext != oldWidget.context?.sampleContext) {
      _schedulePreview();
    }
  }

  @override
  void dispose() {
    _previewTimer?.cancel();
    _controller.removeListener(_handleChanged);
    _controller.dispose();
    super.dispose();
  }

  bool get _hasSample => widget.context?.sampleContext != null;

  String _wdlFromLowered(String jsonText) {
    try {
      final parsed = jsonDecode(jsonText.isEmpty ? 'null' : jsonText);
      return expressionJsonToWdl(parsed);
    } catch (_) {
      return jsonText;
    }
  }

  void _handleChanged() {
    try {
      final lowered = jsonEncode(parseWdlExpression(_controller.text));
      _parseError = '';
      _lastEmitted = lowered;
      widget.onChanged(lowered);
      _schedulePreview();
    } catch (err) {
      setState(() => _parseError = err.toString());
    }
  }

  void _schedulePreview() {
    _previewTimer?.cancel();
    if (!_hasSample) {
      setState(() {
        _previewResult = '';
        _previewError = '';
        _previewUnresolved = '';
      });
      return;
    }

    _previewTimer = Timer(const Duration(milliseconds: 250), _runPreview);
  }

  Future<void> _runPreview() async {
    final sample = widget.context?.sampleContext;
    if (sample == null) return;

    Object? expression;
    try {
      expression = jsonDecode(widget.value.isEmpty ? 'null' : widget.value);
    } catch (_) {
      return;
    }

    final token = ++_previewToken;
    try {
      final resolved = await ref.read(expressionServiceProvider).evaluateSilent(expression, sample);
      if (!mounted || token != _previewToken) return;
      setState(() {
        _previewError = '';
        _previewUnresolved = '';
        _previewResult = pretty(resolved);
      });
    } catch (err) {
      if (!mounted || token != _previewToken) return;
      final message = err.toString();
      setState(() {
        _previewResult = '';
        if (message.contains('WORKFLOW017') || message.contains('unresolved')) {
          _previewError = '';
          _previewUnresolved = 'Not available in this preview (resolves at runtime).';
        } else {
          _previewError = message;
          _previewUnresolved = '';
        }
      });
    }
  }

  void _insertReference(String text) {
    final value = _controller.value;
    final start = value.selection.start >= 0 ? value.selection.start : _controller.text.length;
    final end = value.selection.end >= 0 ? value.selection.end : start;
    final next = _controller.text.replaceRange(start, end, text);
    _controller.value = TextEditingValue(text: next, selection: TextSelection.collapsed(offset: start + text.length));
  }

  void _applyTransform(String kind) {
    final value = _controller.value;
    final start = value.selection.start >= 0 ? value.selection.start : 0;
    final end = value.selection.end >= 0 ? value.selection.end : _controller.text.length;
    final selected = _controller.text.substring(start, end);
    final insert = switch (kind) {
      'string' => 'string($selected)',
      'json' => 'json($selected)',
      'coalesce' => '$selected ?? ',
      _ => '$selected ++ ',
    };
    _controller.value = TextEditingValue(text: _controller.text.replaceRange(start, end, insert), selection: TextSelection.collapsed(offset: start + insert.length));
  }

  @override
  Widget build(BuildContext context) {
    final groups = workflowReferenceGroups(widget.context);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(child: Text(widget.title, style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 12))),
            if (!widget.readOnly)
              TextButton(
                onPressed: () => setState(() => _showPicker = !_showPicker),
                child: Text(_showPicker ? 'Hide references' : 'Insert reference'),
              ),
          ],
        ),
        CodeEditor(
          controller: _controller,
          value: _controller.text,
          onChanged: (_) {},
          readOnly: widget.readOnly,
          language: TextEditorLanguage.wdl,
          minLines: 4,
        ),
        if (_showPicker && !widget.readOnly) ...[
          const SizedBox(height: 8),
          ReferencePicker(groups: groups, onInsert: _insertReference, onTransform: _applyTransform),
        ],
        if (_hasSample) ...[
          const SizedBox(height: 8),
          ExpansionTile(
            tilePadding: EdgeInsets.zero,
            title: const Text('Resolved against last run', style: TextStyle(fontSize: 12)),
            initiallyExpanded: true,
            children: [
              SelectableText(
                _previewUnresolved.isNotEmpty
                    ? _previewUnresolved
                    : (_previewError.isNotEmpty ? _previewError : (_previewResult.isEmpty ? '—' : _previewResult)),
                style: TextStyle(
                  fontFamily: kMonoFontFamily,
                  fontFamilyFallback: kMonoFontFamilyFallback,
                  fontSize: 11,
                  color: _previewError.isNotEmpty ? AppColors.dangerFg : AppColors.textSubtle,
                ),
              ),
            ],
          ),
        ],
        ExpansionTile(
          tilePadding: EdgeInsets.zero,
          title: const Text('Lowered value', style: TextStyle(fontSize: 12)),
          children: [
            SelectableText(widget.value, style: const TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 11)),
          ],
        ),
        if (_parseError.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Text(_parseError, style: TextStyle(color: AppColors.dangerFg, fontSize: 11)),
          ),
      ],
    );
  }
}
