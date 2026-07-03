// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'org_resources_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(orgResourcesService)
final orgResourcesServiceProvider = OrgResourcesServiceProvider._();

final class OrgResourcesServiceProvider
    extends
        $FunctionalProvider<
          OrgResourcesService,
          OrgResourcesService,
          OrgResourcesService
        >
    with $Provider<OrgResourcesService> {
  OrgResourcesServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'orgResourcesServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$orgResourcesServiceHash();

  @$internal
  @override
  $ProviderElement<OrgResourcesService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  OrgResourcesService create(Ref ref) {
    return orgResourcesService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(OrgResourcesService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<OrgResourcesService>(value),
    );
  }
}

String _$orgResourcesServiceHash() =>
    r'531eef06b6f1c59ed44e91c124727656a5158d44';
