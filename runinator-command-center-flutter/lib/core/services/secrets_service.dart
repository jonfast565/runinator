// port of core/services/secrets.ts.

import 'dart:convert';

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import '../utils/secrets.dart';
import '../workflow/editor_defaults.dart' show boundedIndex;
import 'app_service.dart';

part 'secrets_service.g.dart';

class SecretDraft {
  const SecretDraft({required this.scope, required this.name, required this.secret, required this.kind});

  final String scope;
  final String name;
  final String secret;
  final SettingKind kind;
}

SecretDraft blankSecretDraft([SettingKind kind = SettingKind.secret]) =>
    SecretDraft(scope: '', name: '', secret: '', kind: kind);

class SecretsState {
  const SecretsState({required this.secrets, required this.configValues, required this.selectedSecretKey});

  final List<CredentialSummary> secrets;
  final Map<String, String> configValues;
  final String selectedSecretKey;

  SecretsState copyWith({
    List<CredentialSummary>? secrets,
    Map<String, String>? configValues,
    String? selectedSecretKey,
  }) =>
      SecretsState(
        secrets: secrets ?? this.secrets,
        configValues: configValues ?? this.configValues,
        selectedSecretKey: selectedSecretKey ?? this.selectedSecretKey,
      );
}

@riverpod
class SecretsNotifier extends _$SecretsNotifier {
  @override
  SecretsState build() => const SecretsState(secrets: [], configValues: {}, selectedSecretKey: '');

  CredentialSummary? selectedSecret() {
    for (final secret in state.secrets) {
      if (secretKey(secret) == state.selectedSecretKey) {
        return secret;
      }
    }
    return null;
  }

  List<CredentialSummary> configEntries() =>
      state.secrets.where((secret) => (secret.kind ?? SettingKind.secret) == SettingKind.config).toList();

  List<CredentialSummary> secretEntries() =>
      state.secrets.where((secret) => (secret.kind ?? SettingKind.secret) == SettingKind.secret).toList();

  List<CredentialSummary> filteredSecrets(String query) {
    if (query.isEmpty) {
      return state.secrets;
    }

    return state.secrets
        .where((secret) => [secret.scope, secret.name].any((value) => value.toLowerCase().contains(query)))
        .toList();
  }

  List<String> scopes() {
    final set = state.secrets.map((secret) => secret.scope).toSet().toList()..sort();
    return set;
  }

  List<CredentialSummary> secretsForScopes(List<String> credentialScopes) {
    if (credentialScopes.isEmpty) {
      return state.secrets;
    }

    final allowed = credentialScopes.toSet();
    return state.secrets.where((secret) => allowed.contains(secret.scope)).toList();
  }

  void moveSecretSelection(int delta, String query) {
    final list = filteredSecrets(query);

    if (list.isEmpty) {
      return;
    }

    final current = list.indexWhere((secret) => secretKey(secret) == state.selectedSecretKey);
    selectSecret(list[boundedIndex(current, delta, list.length)]);
  }

  void setSelectedSecretKey(String key) {
    state = state.copyWith(selectedSecretKey: key);
  }

  void selectSecret(CredentialSummary secret) {
    state = state.copyWith(selectedSecretKey: secretKey(secret));
  }

  void clearSelection() {
    state = state.copyWith(selectedSecretKey: '');
  }

  Future<void> refreshSecrets() async {
    final app = ref.read(appProvider.notifier);
    List<CredentialSummary> secrets;
    try {
      secrets = await app.runOperation('Refreshing secrets', api.fetchCredentials);
    } catch (_) {
      secrets = [];
    }
    secrets = [...secrets]
      ..sort((left, right) {
        final scopeCompare = left.scope.compareTo(right.scope);
        return scopeCompare != 0 ? scopeCompare : left.name.compareTo(right.name);
      });

    var selectedSecretKey = state.selectedSecretKey;

    if (selectedSecretKey.isNotEmpty && !secrets.any((secret) => secretKey(secret) == selectedSecretKey)) {
      selectedSecretKey = '';
    }

    if (selectedSecretKey.isEmpty && secrets.isNotEmpty) {
      selectedSecretKey = secretKey(secrets.first);
    }

    state = state.copyWith(secrets: secrets, selectedSecretKey: selectedSecretKey);
  }

  void clearSecrets() {
    state = const SecretsState(secrets: [], configValues: {}, selectedSecretKey: '');
  }

  Future<void> loadConfigValue(CredentialSummary setting) async {
    if ((setting.kind ?? SettingKind.secret) != SettingKind.config) {
      return;
    }

    final app = ref.read(appProvider.notifier);
    final key = secretKey(setting);
    final detail = await app.runOperation(
      'Loading config value',
      () => api.fetchCredential(setting.scope, setting.name, SettingKind.config),
    );
    state = state.copyWith(
      configValues: {...state.configValues, key: _formatConfigValue(detail.value ?? detail.secret)},
    );
  }

  Future<void> loadConfigValues(List<CredentialSummary> settings) async {
    await Future.wait(
      settings.where((s) => (s.kind ?? SettingKind.secret) == SettingKind.config).map(loadConfigValue),
    );
  }

  Future<bool> saveDraft(SecretDraft draft) async {
    final app = ref.read(appProvider.notifier);
    final scope = draft.scope.trim();
    final name = draft.name.trim();
    final kind = draft.kind;
    final label = kind == SettingKind.config ? 'Config' : 'Secret';

    if (scope.isEmpty || name.isEmpty || draft.secret.trim().isEmpty) {
      app.setError('$label scope, name, and value are required');
      return false;
    }

    Object? value = draft.secret;

    if (kind == SettingKind.config) {
      try {
        value = jsonDecode(draft.secret);
      } catch (_) {
        app.setError('Config value must be valid JSON');
        return false;
      }
    }

    await app.runOperation('Saving ${kind.wire}', () => api.saveCredential(scope, name, value, kind));
    state = state.copyWith(
      selectedSecretKey: secretKey(CredentialSummary(scope: scope, name: name, kind: kind)),
    );
    app.setStatus('$label saved: $scope/$name');
    await refreshSecrets();
    return true;
  }

  Future<void> deleteSelectedSecret() async {
    final app = ref.read(appProvider.notifier);
    final secret = selectedSecret();

    if (secret == null) {
      app.setError('No setting selected');
      return;
    }

    final kind = secret.kind ?? SettingKind.secret;
    await app.runOperation('Deleting ${kind.wire}', () => api.deleteCredential(secret.scope, secret.name, kind));
    app.setStatus('${kind == SettingKind.config ? 'Config' : 'Secret'} deleted: ${secret.scope}/${secret.name}');
    state = state.copyWith(selectedSecretKey: '');
    await refreshSecrets();
  }
}

String _formatConfigValue(Object? value) {
  if (value == null) {
    return '';
  }

  return const JsonEncoder.withIndent('  ').convert(value);
}
