// port of core/platform/text-editor.ts.
//
// `HTMLElement` (the mount() container in the ts source) has no
// Flutter-independent equivalent; represented as an opaque `Object` here — the
// future UI pass's CodeMirror-equivalent adapter narrows it to whatever concrete
// widget/handle it actually needs.

enum TextEditorLanguage {
  wdl('wdl'),
  json('json'),
  expression('expression');

  const TextEditorLanguage(this.wire);

  final String wire;
}

enum TextEditorDiagnosticSeverity {
  error('error'),
  warning('warning'),
  info('info');

  const TextEditorDiagnosticSeverity(this.wire);

  final String wire;
}

class TextEditorDiagnostic {
  const TextEditorDiagnostic({
    required this.severity,
    required this.message,
    required this.line,
    required this.column,
  });

  final TextEditorDiagnosticSeverity severity;
  final String message;
  final int line;
  final int column;
}

class TextEditorHostCreateOptions {
  const TextEditorHostCreateOptions({
    required this.language,
    required this.value,
    this.readonly = false,
    this.sourcePath,
    required this.onChange,
    this.onDiagnosticsChange,
  });

  final TextEditorLanguage language;
  final String value;
  final bool readonly;
  final String? sourcePath;
  final void Function(String value) onChange;
  final void Function(List<TextEditorDiagnostic> diagnostics)? onDiagnosticsChange;
}

/// framework-neutral editor surface; a code_text_field-based adapter implements
/// this in the future UI pass (mirrors the ts source's CodeMirror adapter).
abstract class TextEditorHost {
  void mount(Object container);

  void destroy();

  String getValue();

  void setValue(String value, {bool silent = false});

  void setReadonly(bool readonly);

  void focus();

  void goToPosition(int line, [int? column]);

  List<TextEditorDiagnostic> getDiagnostics();

  Future<void> formatDocument() async {}
}

abstract class TextEditorHostFactory {
  TextEditorHost create(TextEditorHostCreateOptions options);
}
