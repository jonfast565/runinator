import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/providers_service.dart';
import '../../core/services/secrets_service.dart';
import '../../core/services/wdl_language_service.dart';
import '../adapters/wdl_text_utils.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../theme/app_theme.dart';

class WdlEditorPanel extends ConsumerStatefulWidget {
  const WdlEditorPanel({
    super.key,
    required this.value,
    required this.onChanged,
    this.readOnly = false,
    this.sourcePath,
  });

  final String value;
  final ValueChanged<String> onChanged;
  final bool readOnly;
  final String? sourcePath;

  @override
  ConsumerState<WdlEditorPanel> createState() => _WdlEditorPanelState();
}

class _WdlEditorPanelState extends ConsumerState<WdlEditorPanel> {
  List<WdlDiagnostic> _diagnostics = const [];
  Timer? _analyzeTimer;
  var _formatting = false;

  @override
  void dispose() {
    _analyzeTimer?.cancel();
    super.dispose();
  }

  void _scheduleAnalyze(String source) {
    _analyzeTimer?.cancel();
    _analyzeTimer = Timer(const Duration(milliseconds: 1500), () async {
      final service = ref.read(wdlLanguageServiceProvider);
      try {
        final diagnostics = await service.analyzeSilent(source, widget.sourcePath);
        if (mounted) setState(() => _diagnostics = diagnostics);
      } catch (_) {
        if (mounted) setState(() => _diagnostics = const []);
      }
    });
  }

  Future<void> _format() async {
    setState(() => _formatting = true);
    try {
      final formatted = await ref.read(wdlLanguageServiceProvider).formatSilent(widget.value);
      widget.onChanged(formatted);
    } finally {
      if (mounted) setState(() => _formatting = false);
    }
  }

  Future<List<WdlCompletionSuggestion>> _complete(int cursor, String source) async {
    final providers = ref.read(providersProvider).providers;
    final settings = settingRefsFromCredentials(ref.read(secretsProvider).secrets);
    final service = ref.read(wdlLanguageServiceProvider);
    try {
      final response = await service.complete(WdlCompletionRequest(
        source: source,
        cursorByte: utf16OffsetToUtf8ByteOffset(source, cursor),
        providers: providers,
        settings: settings,
      ));
      return response.items
          .map((item) => WdlCompletionSuggestion(label: item.label, insertText: item.insertText, detail: item.detail))
          .toList();
    } catch (_) {
      return const [];
    }
  }

  Future<WdlHoverInfo?> _hover(int cursor, String source) async {
    final providers = ref.read(providersProvider).providers;
    final settings = settingRefsFromCredentials(ref.read(secretsProvider).secrets);
    final service = ref.read(wdlLanguageServiceProvider);
    try {
      final response = await service.hover(WdlHoverRequest(
        source: source,
        cursorByte: utf16OffsetToUtf8ByteOffset(source, cursor),
        providers: providers,
        settings: settings,
      ));
      if (response == null) return null;
      return WdlHoverInfo(title: response.title, documentation: response.documentation);
    } catch (_) {
      return null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final errorCount = _diagnostics.where((d) => d.severity == WdlDiagnosticSeverity.error).length;
    final warnCount = _diagnostics.where((d) => d.severity == WdlDiagnosticSeverity.warning).length;
    final summary = errorCount > 0
        ? '$errorCount error${errorCount == 1 ? '' : 's'}'
        : warnCount > 0
            ? '$warnCount warning${warnCount == 1 ? '' : 's'}'
            : 'Clean';

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            const Text('WDL', style: TextStyle(fontWeight: FontWeight.w700)),
            const SizedBox(width: 12),
            StatusBadge(summary),
            const Spacer(),
            CcButton(
              icon: IconName.save,
              label: _formatting ? 'Formatting…' : 'Format',
              dense: true,
              onPressed: widget.readOnly || _formatting ? null : _format,
            ),
          ],
        ),
        const SizedBox(height: 8),
        Expanded(
          child: WdlEditor(
            value: widget.value,
            readOnly: widget.readOnly,
            onComplete: _complete,
            onHover: _hover,
            onChanged: (value) {
              widget.onChanged(value);
              _scheduleAnalyze(value);
            },
          ),
        ),
        if (_diagnostics.isNotEmpty) ...[
          const SizedBox(height: 8),
          Container(
            constraints: const BoxConstraints(maxHeight: 120),
            decoration: BoxDecoration(
              border: Border.all(color: AppColors.border),
              borderRadius: BorderRadius.circular(6),
            ),
            child: ListView.builder(
              itemCount: _diagnostics.length,
              itemBuilder: (context, index) {
                final d = _diagnostics[index];
                return ListTile(
                  dense: true,
                  title: Text(d.message, style: const TextStyle(fontSize: 11)),
                  subtitle: Text('Line ${d.line}, col ${d.column}', style: const TextStyle(fontSize: 10)),
                  leading: Icon(
                    d.severity == WdlDiagnosticSeverity.error ? Icons.error_outline : Icons.warning_amber,
                    size: 14,
                    color: d.severity == WdlDiagnosticSeverity.error ? AppColors.dangerFg : AppColors.warningFg,
                  ),
                );
              },
            ),
          ),
        ],
      ],
    );
  }
}
