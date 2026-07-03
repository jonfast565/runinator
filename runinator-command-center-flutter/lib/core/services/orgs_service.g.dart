// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'orgs_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(OrgsNotifier)
final orgsProvider = OrgsNotifierProvider._();

final class OrgsNotifierProvider
    extends $NotifierProvider<OrgsNotifier, OrgsState> {
  OrgsNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'orgsProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$orgsNotifierHash();

  @$internal
  @override
  OrgsNotifier create() => OrgsNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(OrgsState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<OrgsState>(value),
    );
  }
}

String _$orgsNotifierHash() => r'8d882dbe86635ca0db8d1de7e36fa5fd1a3c9706';

abstract class _$OrgsNotifier extends $Notifier<OrgsState> {
  OrgsState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<OrgsState, OrgsState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<OrgsState, OrgsState>,
              OrgsState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
