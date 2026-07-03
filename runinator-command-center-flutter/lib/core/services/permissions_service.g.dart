// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'permissions_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(PermissionsNotifier)
final permissionsProvider = PermissionsNotifierProvider._();

final class PermissionsNotifierProvider
    extends $NotifierProvider<PermissionsNotifier, PermissionsState> {
  PermissionsNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'permissionsProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$permissionsNotifierHash();

  @$internal
  @override
  PermissionsNotifier create() => PermissionsNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(PermissionsState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<PermissionsState>(value),
    );
  }
}

String _$permissionsNotifierHash() =>
    r'32425ff02c5addebf0e5eb83958e7b00a015e276';

abstract class _$PermissionsNotifier extends $Notifier<PermissionsState> {
  PermissionsState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<PermissionsState, PermissionsState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<PermissionsState, PermissionsState>,
              PermissionsState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
