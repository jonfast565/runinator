// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'wdl_language_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(wdlLanguageService)
final wdlLanguageServiceProvider = WdlLanguageServiceProvider._();

final class WdlLanguageServiceProvider
    extends
        $FunctionalProvider<
          WdlLanguageService,
          WdlLanguageService,
          WdlLanguageService
        >
    with $Provider<WdlLanguageService> {
  WdlLanguageServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'wdlLanguageServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$wdlLanguageServiceHash();

  @$internal
  @override
  $ProviderElement<WdlLanguageService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  WdlLanguageService create(Ref ref) {
    return wdlLanguageService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(WdlLanguageService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<WdlLanguageService>(value),
    );
  }
}

String _$wdlLanguageServiceHash() =>
    r'47c7286ab2735e0f8ebe32bd83f40fff5d64899c';
