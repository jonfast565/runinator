// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'admin_settings_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(AdminSettingsNotifier)
final adminSettingsProvider = AdminSettingsNotifierProvider._();

final class AdminSettingsNotifierProvider
    extends $NotifierProvider<AdminSettingsNotifier, AdminSettingsState> {
  AdminSettingsNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'adminSettingsProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$adminSettingsNotifierHash();

  @$internal
  @override
  AdminSettingsNotifier create() => AdminSettingsNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(AdminSettingsState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<AdminSettingsState>(value),
    );
  }
}

String _$adminSettingsNotifierHash() =>
    r'705af1964a7ed3fcbd62c560aa7f78978050b5a2';

abstract class _$AdminSettingsNotifier extends $Notifier<AdminSettingsState> {
  AdminSettingsState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<AdminSettingsState, AdminSettingsState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<AdminSettingsState, AdminSettingsState>,
              AdminSettingsState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
