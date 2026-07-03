// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'gates_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(GatesNotifier)
final gatesProvider = GatesNotifierProvider._();

final class GatesNotifierProvider
    extends $NotifierProvider<GatesNotifier, GatesState> {
  GatesNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'gatesProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$gatesNotifierHash();

  @$internal
  @override
  GatesNotifier create() => GatesNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(GatesState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<GatesState>(value),
    );
  }
}

String _$gatesNotifierHash() => r'ac0744efcf4f23c140e03adb5cd460bebe895e22';

abstract class _$GatesNotifier extends $Notifier<GatesState> {
  GatesState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<GatesState, GatesState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<GatesState, GatesState>,
              GatesState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
