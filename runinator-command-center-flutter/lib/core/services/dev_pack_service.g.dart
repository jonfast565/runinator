// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'dev_pack_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

@ProviderFor(devPackService)
final devPackServiceProvider = DevPackServiceProvider._();

final class DevPackServiceProvider extends $FunctionalProvider<DevPackService, DevPackService, DevPackService>
    with $Provider<DevPackService> {
  DevPackServiceProvider._()
      : super(
          from: null,
          argument: null,
          retry: null,
          name: r'devPackServiceProvider',
          isAutoDispose: true,
          dependencies: null,
          $allTransitiveDependencies: null,
        );

  @override
  String debugGetCreateSourceHash() => _$devPackServiceHash();

  @$internal
  @override
  $ProviderElement<DevPackService> $createElement($ProviderPointer pointer) => $ProviderElement(pointer);

  @override
  DevPackService create(Ref ref) => devPackService(ref);

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(DevPackService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<DevPackService>(value),
    );
  }
}

String _$devPackServiceHash() => r'dev_pack_service_hash';
