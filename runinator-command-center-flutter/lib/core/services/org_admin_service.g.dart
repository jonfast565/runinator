// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'org_admin_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(orgAdminService)
final orgAdminServiceProvider = OrgAdminServiceProvider._();

final class OrgAdminServiceProvider
    extends
        $FunctionalProvider<OrgAdminService, OrgAdminService, OrgAdminService>
    with $Provider<OrgAdminService> {
  OrgAdminServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'orgAdminServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$orgAdminServiceHash();

  @$internal
  @override
  $ProviderElement<OrgAdminService> $createElement($ProviderPointer pointer) =>
      $ProviderElement(pointer);

  @override
  OrgAdminService create(Ref ref) {
    return orgAdminService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(OrgAdminService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<OrgAdminService>(value),
    );
  }
}

String _$orgAdminServiceHash() => r'fbe94e758eea911cb06030a4d71240e64bc64299';
