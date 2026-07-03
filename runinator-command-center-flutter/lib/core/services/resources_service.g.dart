// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'resources_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(ResourcesNotifier)
final resourcesProvider = ResourcesNotifierProvider._();

final class ResourcesNotifierProvider
    extends $NotifierProvider<ResourcesNotifier, ResourcesState> {
  ResourcesNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'resourcesProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$resourcesNotifierHash();

  @$internal
  @override
  ResourcesNotifier create() => ResourcesNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(ResourcesState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<ResourcesState>(value),
    );
  }
}

String _$resourcesNotifierHash() => r'142ac6d4b862537b6188134723330eb5508090c0';

abstract class _$ResourcesNotifier extends $Notifier<ResourcesState> {
  ResourcesState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<ResourcesState, ResourcesState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<ResourcesState, ResourcesState>,
              ResourcesState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
