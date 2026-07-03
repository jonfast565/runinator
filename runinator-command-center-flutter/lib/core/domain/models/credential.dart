// port of core/domain/models/credential.ts.

import '../json.dart';
import 'setting.dart';

class CredentialSummary {
  const CredentialSummary({required this.scope, required this.name, this.kind});

  factory CredentialSummary.fromJson(Map<String, Object?> json) => CredentialSummary(
        scope: json['scope'] as String,
        name: json['name'] as String,
        kind: json['kind'] != null ? SettingKind.fromJson(json['kind'] as String) : null,
      );

  final String scope;
  final String name;
  final SettingKind? kind;

  Map<String, Object?> toJson() => {'scope': scope, 'name': name, 'kind': kind?.toJson()};
}

class CredentialDetail extends CredentialSummary {
  const CredentialDetail({
    required super.scope,
    required super.name,
    super.kind,
    this.value,
    this.secret,
  });

  factory CredentialDetail.fromJson(Map<String, Object?> json) => CredentialDetail(
        scope: json['scope'] as String,
        name: json['name'] as String,
        kind: json['kind'] != null ? SettingKind.fromJson(json['kind'] as String) : null,
        value: json.containsKey('value') ? asJsonValue(json['value']) : null,
        secret: json.containsKey('secret') ? asJsonValue(json['secret']) : null,
      );

  final JsonValue value;
  final JsonValue secret;

  @override
  Map<String, Object?> toJson() => {...super.toJson(), 'value': value, 'secret': secret};
}
