// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'artifacts_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(ArtifactsNotifier)
final artifactsProvider = ArtifactsNotifierProvider._();

final class ArtifactsNotifierProvider
    extends $NotifierProvider<ArtifactsNotifier, ArtifactsState> {
  ArtifactsNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'artifactsProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$artifactsNotifierHash();

  @$internal
  @override
  ArtifactsNotifier create() => ArtifactsNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(ArtifactsState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<ArtifactsState>(value),
    );
  }
}

String _$artifactsNotifierHash() => r'e5ffc1b4ae9e151d4b07baf89da3730f840e1860';

abstract class _$ArtifactsNotifier extends $Notifier<ArtifactsState> {
  ArtifactsState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<ArtifactsState, ArtifactsState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<ArtifactsState, ArtifactsState>,
              ArtifactsState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
