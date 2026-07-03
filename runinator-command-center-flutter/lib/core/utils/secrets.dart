// port of core/utils/secrets.ts.

import '../domain/models/index.dart';

const String secretRefPrefix = 'secret://';

String secretKey(CredentialSummary secret) => '${secret.kind?.wire ?? 'secret'}:${secret.scope}:${secret.name}';

String secretRef(String scope, String name) =>
    '$secretRefPrefix${Uri.encodeComponent(scope)}/${Uri.encodeComponent(name)}';

/// wdl-style reference for a setting, e.g. `secret.github.token` or `config.api.url`.
String settingRef(SettingKind? kind, String scope, String name) => '${kind?.wire ?? 'secret'}.$scope.$name';

CredentialSummary? parseSecretRef(Object? value) {
  if (value is! String || !value.startsWith(secretRefPrefix)) {
    return null;
  }

  final path = value.substring(secretRefPrefix.length);
  final parts = path.split('/');

  if (parts.length < 2 || parts[0].isEmpty || parts[1].isEmpty) {
    return null;
  }

  return CredentialSummary(scope: Uri.decodeComponent(parts[0]), name: Uri.decodeComponent(parts[1]));
}

String secretRefLabel(Object? value) {
  final parsed = parseSecretRef(value);
  return parsed != null ? '${parsed.scope}/${parsed.name}' : '';
}
