// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'dead_letters_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(deadLettersService)
final deadLettersServiceProvider = DeadLettersServiceProvider._();

final class DeadLettersServiceProvider
    extends
        $FunctionalProvider<
          DeadLettersService,
          DeadLettersService,
          DeadLettersService
        >
    with $Provider<DeadLettersService> {
  DeadLettersServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'deadLettersServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$deadLettersServiceHash();

  @$internal
  @override
  $ProviderElement<DeadLettersService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  DeadLettersService create(Ref ref) {
    return deadLettersService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(DeadLettersService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<DeadLettersService>(value),
    );
  }
}

String _$deadLettersServiceHash() =>
    r'94031f089d9d69c91018323783d4ec8fe9f2a44a';
