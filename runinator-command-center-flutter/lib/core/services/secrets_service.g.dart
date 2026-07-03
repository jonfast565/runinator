// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'secrets_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(SecretsNotifier)
final secretsProvider = SecretsNotifierProvider._();

final class SecretsNotifierProvider
    extends $NotifierProvider<SecretsNotifier, SecretsState> {
  SecretsNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'secretsProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$secretsNotifierHash();

  @$internal
  @override
  SecretsNotifier create() => SecretsNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(SecretsState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<SecretsState>(value),
    );
  }
}

String _$secretsNotifierHash() => r'8ff952cb5cdd222deb5db4c4805aca8880b10a45';

abstract class _$SecretsNotifier extends $Notifier<SecretsState> {
  SecretsState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<SecretsState, SecretsState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<SecretsState, SecretsState>,
              SecretsState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
