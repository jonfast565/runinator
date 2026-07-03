// port of core/domain/models/dev-pack.ts.

import '../json.dart';
import 'wdl/wdl.dart';
import 'workflow/bundle.dart';
import 'workflow/definition.dart';
import 'workflow/trigger.dart';

class DevPackFile {
  const DevPackFile({required this.path, required this.kind, this.sizeBytes, this.modifiedAt});

  factory DevPackFile.fromJson(Map<String, Object?> json) => DevPackFile(
        path: json['path'] as String,
        kind: json['kind'] as String,
        sizeBytes: (json['size_bytes'] as num?)?.toInt(),
        modifiedAt: json['modified_at'] as String?,
      );

  final String path;
  final String kind;
  final int? sizeBytes;
  final String? modifiedAt;

  Map<String, Object?> toJson() => {
        'path': path,
        'kind': kind,
        'size_bytes': sizeBytes,
        'modified_at': modifiedAt,
      };
}

class DevPackInspectResult {
  const DevPackInspectResult({
    required this.path,
    required this.files,
    required this.workflows,
    required this.triggers,
    required this.settingsCount,
    required this.settings,
  });

  factory DevPackInspectResult.fromJson(Map<String, Object?> json) => DevPackInspectResult(
        path: json['path'] as String,
        files: (json['files'] as List)
            .map((f) => DevPackFile.fromJson(f as Map<String, Object?>))
            .toList(),
        workflows: (json['workflows'] as List)
            .map((w) => WorkflowDefinition.fromJson(w as Map<String, Object?>))
            .toList(),
        triggers: (json['triggers'] as List)
            .map((t) => WorkflowTrigger.fromJson(t as Map<String, Object?>))
            .toList(),
        settingsCount: (json['settings_count'] as num).toInt(),
        settings: (json['settings'] as List)
            .map((s) => WdlSettingRef.fromJson(s as Map<String, Object?>))
            .toList(),
      );

  final String path;
  final List<DevPackFile> files;
  final List<WorkflowDefinition> workflows;
  final List<WorkflowTrigger> triggers;
  final int settingsCount;
  final List<WdlSettingRef> settings;

  Map<String, Object?> toJson() => {
        'path': path,
        'files': files.map((f) => f.toJson()).toList(),
        'workflows': workflows.map((w) => w.toJson()).toList(),
        'triggers': triggers.map((t) => t.toJson()).toList(),
        'settings_count': settingsCount,
        'settings': settings.map((s) => s.toJson()).toList(),
      };
}

class DevPackTextFile {
  const DevPackTextFile({required this.path, required this.content, this.modifiedAt});

  factory DevPackTextFile.fromJson(Map<String, Object?> json) => DevPackTextFile(
        path: json['path'] as String,
        content: json['content'] as String,
        modifiedAt: json['modified_at'] as String?,
      );

  final String path;
  final String content;
  final String? modifiedAt;

  Map<String, Object?> toJson() => {'path': path, 'content': content, 'modified_at': modifiedAt};
}

class DevPackApplyResultSecrets {
  const DevPackApplyResultSecrets({this.secrets});

  factory DevPackApplyResultSecrets.fromJson(Map<String, Object?> json) =>
      DevPackApplyResultSecrets(
        secrets: json['secrets'] != null ? asJsonArray(json['secrets']) : null,
      );

  final JsonArray? secrets;

  Map<String, Object?> toJson() => {'secrets': secrets};
}

class DevPackApplyResultImported {
  const DevPackApplyResultImported({required this.workflows, this.secrets});

  factory DevPackApplyResultImported.fromJson(Map<String, Object?> json) =>
      DevPackApplyResultImported(
        workflows: WorkflowBundle.fromJson(json['workflows'] as Map<String, Object?>),
        secrets: json['secrets'] != null
            ? DevPackApplyResultSecrets.fromJson(json['secrets'] as Map<String, Object?>)
            : null,
      );

  final WorkflowBundle workflows;
  final DevPackApplyResultSecrets? secrets;

  Map<String, Object?> toJson() => {
        'workflows': workflows.toJson(),
        'secrets': secrets?.toJson(),
      };
}

class DevPackApplyResult {
  const DevPackApplyResult({required this.path, required this.files, required this.imported});

  factory DevPackApplyResult.fromJson(Map<String, Object?> json) => DevPackApplyResult(
        path: json['path'] as String,
        files: (json['files'] as List)
            .map((f) => DevPackFile.fromJson(f as Map<String, Object?>))
            .toList(),
        imported: DevPackApplyResultImported.fromJson(json['imported'] as Map<String, Object?>),
      );

  final String path;
  final List<DevPackFile> files;
  final DevPackApplyResultImported imported;

  Map<String, Object?> toJson() => {
        'path': path,
        'files': files.map((f) => f.toJson()).toList(),
        'imported': imported.toJson(),
      };
}
