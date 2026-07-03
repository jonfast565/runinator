// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'providers_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(ProvidersNotifier)
final providersProvider = ProvidersNotifierProvider._();

final class ProvidersNotifierProvider
    extends $NotifierProvider<ProvidersNotifier, ProvidersState> {
  ProvidersNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'providersProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$providersNotifierHash();

  @$internal
  @override
  ProvidersNotifier create() => ProvidersNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(ProvidersState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<ProvidersState>(value),
    );
  }
}

String _$providersNotifierHash() => r'a98ab674bb933a8334a0a5e8d1da5f03cee4a9ab';

abstract class _$ProvidersNotifier extends $Notifier<ProvidersState> {
  ProvidersState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<ProvidersState, ProvidersState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<ProvidersState, ProvidersState>,
              ProvidersState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
