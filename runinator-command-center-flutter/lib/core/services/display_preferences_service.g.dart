// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'display_preferences_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(DisplayPreferencesNotifier)
final displayPreferencesProvider = DisplayPreferencesNotifierProvider._();

final class DisplayPreferencesNotifierProvider
    extends
        $NotifierProvider<DisplayPreferencesNotifier, DisplayPreferencesState> {
  DisplayPreferencesNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'displayPreferencesProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$displayPreferencesNotifierHash();

  @$internal
  @override
  DisplayPreferencesNotifier create() => DisplayPreferencesNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(DisplayPreferencesState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<DisplayPreferencesState>(value),
    );
  }
}

String _$displayPreferencesNotifierHash() =>
    r'4b76deaf0f5d79261265c747fea8241025615b46';

abstract class _$DisplayPreferencesNotifier
    extends $Notifier<DisplayPreferencesState> {
  DisplayPreferencesState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref =
        this.ref as $Ref<DisplayPreferencesState, DisplayPreferencesState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<DisplayPreferencesState, DisplayPreferencesState>,
              DisplayPreferencesState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
