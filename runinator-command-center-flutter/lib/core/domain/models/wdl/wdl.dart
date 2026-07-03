// port of core/domain/models/wdl/wdl.ts.

import '../provider/provider_metadata.dart' show ProviderMetadata;
import '../setting.dart';

enum WdlDiagnosticSeverity {
  error('error'),
  warning('warning');

  const WdlDiagnosticSeverity(this.wire);

  final String wire;

  static WdlDiagnosticSeverity fromJson(String value) => WdlDiagnosticSeverity.values.firstWhere(
        (severity) => severity.wire == value,
        orElse: () => throw ArgumentError('unknown WdlDiagnosticSeverity: $value'),
      );

  String toJson() => wire;
}

class WdlDiagnostic {
  const WdlDiagnostic({
    required this.start,
    required this.end,
    required this.line,
    required this.column,
    required this.severity,
    required this.message,
  });

  factory WdlDiagnostic.fromJson(Map<String, Object?> json) => WdlDiagnostic(
        start: (json['start'] as num).toInt(),
        end: (json['end'] as num).toInt(),
        line: (json['line'] as num).toInt(),
        column: (json['column'] as num).toInt(),
        severity: WdlDiagnosticSeverity.fromJson(json['severity'] as String),
        message: json['message'] as String,
      );

  final int start;
  final int end;
  final int line;
  final int column;
  final WdlDiagnosticSeverity severity;
  final String message;

  Map<String, Object?> toJson() => {
        'start': start,
        'end': end,
        'line': line,
        'column': column,
        'severity': severity.toJson(),
        'message': message,
      };
}

class WdlSettingRef {
  const WdlSettingRef({required this.scope, required this.name, required this.kind});

  factory WdlSettingRef.fromJson(Map<String, Object?> json) => WdlSettingRef(
        scope: json['scope'] as String,
        name: json['name'] as String,
        kind: SettingKind.fromJson(json['kind'] as String),
      );

  final String scope;
  final String name;
  final SettingKind kind;

  Map<String, Object?> toJson() => {'scope': scope, 'name': name, 'kind': kind.toJson()};
}

class WdlCompletionRequest {
  const WdlCompletionRequest({
    required this.source,
    required this.cursorByte,
    required this.providers,
    required this.settings,
  });

  final String source;
  final int cursorByte;
  final List<ProviderMetadata> providers;
  final List<WdlSettingRef> settings;

  Map<String, Object?> toJson() => {
        'source': source,
        'cursor_byte': cursorByte,
        'providers': providers.map((p) => p.toJson()).toList(),
        'settings': settings.map((s) => s.toJson()).toList(),
      };
}

class WdlCompletionItem {
  const WdlCompletionItem({
    required this.label,
    required this.kind,
    this.detail,
    this.documentation,
    required this.insertText,
    required this.isSnippet,
  });

  factory WdlCompletionItem.fromJson(Map<String, Object?> json) => WdlCompletionItem(
        label: json['label'] as String,
        kind: json['kind'] as String,
        detail: json['detail'] as String?,
        documentation: json['documentation'] as String?,
        insertText: json['insert_text'] as String,
        isSnippet: json['is_snippet'] as bool,
      );

  final String label;
  final String kind;
  final String? detail;
  final String? documentation;
  final String insertText;
  final bool isSnippet;

  Map<String, Object?> toJson() => {
        'label': label,
        'kind': kind,
        'detail': detail,
        'documentation': documentation,
        'insert_text': insertText,
        'is_snippet': isSnippet,
      };
}

class WdlCompletionResponse {
  const WdlCompletionResponse({
    required this.replaceStartByte,
    required this.replaceEndByte,
    required this.items,
  });

  factory WdlCompletionResponse.fromJson(Map<String, Object?> json) => WdlCompletionResponse(
        replaceStartByte: (json['replace_start_byte'] as num).toInt(),
        replaceEndByte: (json['replace_end_byte'] as num).toInt(),
        items: (json['items'] as List)
            .map((i) => WdlCompletionItem.fromJson(i as Map<String, Object?>))
            .toList(),
      );

  final int replaceStartByte;
  final int replaceEndByte;
  final List<WdlCompletionItem> items;

  Map<String, Object?> toJson() => {
        'replace_start_byte': replaceStartByte,
        'replace_end_byte': replaceEndByte,
        'items': items.map((i) => i.toJson()).toList(),
      };
}

class WdlHoverRequest {
  const WdlHoverRequest({
    required this.source,
    required this.cursorByte,
    required this.providers,
    this.settings,
  });

  final String source;
  final int cursorByte;
  final List<ProviderMetadata> providers;
  final List<WdlSettingRef>? settings;

  Map<String, Object?> toJson() => {
        'source': source,
        'cursor_byte': cursorByte,
        'providers': providers.map((p) => p.toJson()).toList(),
        'settings': settings?.map((s) => s.toJson()).toList(),
      };
}

class WdlHoverResponse {
  const WdlHoverResponse({
    required this.rangeStartByte,
    required this.rangeEndByte,
    required this.title,
    required this.kind,
    this.detail,
    this.documentation,
  });

  factory WdlHoverResponse.fromJson(Map<String, Object?> json) => WdlHoverResponse(
        rangeStartByte: (json['range_start_byte'] as num).toInt(),
        rangeEndByte: (json['range_end_byte'] as num).toInt(),
        title: json['title'] as String,
        kind: json['kind'] as String,
        detail: json['detail'] as String?,
        documentation: json['documentation'] as String?,
      );

  final int rangeStartByte;
  final int rangeEndByte;
  final String title;
  final String kind;
  final String? detail;
  final String? documentation;

  Map<String, Object?> toJson() => {
        'range_start_byte': rangeStartByte,
        'range_end_byte': rangeEndByte,
        'title': title,
        'kind': kind,
        'detail': detail,
        'documentation': documentation,
      };
}
