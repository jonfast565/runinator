// port of core/domain/models/provider/provider-metadata.ts.

import 'action_metadata.dart';

class ProviderRuntimeMetadata {
  const ProviderRuntimeMetadata({required this.credentialScopes, this.contract});

  factory ProviderRuntimeMetadata.fromJson(Map<String, Object?> json) => ProviderRuntimeMetadata(
        credentialScopes: (json['credential_scopes'] as List).cast<String>(),
        contract: json['contract'] as String?,
      );

  final List<String> credentialScopes;
  final String? contract;

  Map<String, Object?> toJson() => {
        'credential_scopes': credentialScopes,
        'contract': contract,
      };
}

class ProviderMetadata {
  const ProviderMetadata({required this.name, required this.actions, required this.metadata});

  factory ProviderMetadata.fromJson(Map<String, Object?> json) => ProviderMetadata(
        name: json['name'] as String,
        actions: (json['actions'] as List)
            .map((a) => ActionMetadata.fromJson(a as Map<String, Object?>))
            .toList(),
        metadata: ProviderRuntimeMetadata.fromJson(json['metadata'] as Map<String, Object?>),
      );

  final String name;
  final List<ActionMetadata> actions;
  final ProviderRuntimeMetadata metadata;

  Map<String, Object?> toJson() => {
        'name': name,
        'actions': actions.map((a) => a.toJson()).toList(),
        'metadata': metadata.toJson(),
      };
}
