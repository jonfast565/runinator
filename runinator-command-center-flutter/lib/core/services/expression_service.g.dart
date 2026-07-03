// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'expression_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(expressionService)
final expressionServiceProvider = ExpressionServiceProvider._();

final class ExpressionServiceProvider
    extends
        $FunctionalProvider<
          ExpressionService,
          ExpressionService,
          ExpressionService
        >
    with $Provider<ExpressionService> {
  ExpressionServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'expressionServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$expressionServiceHash();

  @$internal
  @override
  $ProviderElement<ExpressionService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  ExpressionService create(Ref ref) {
    return expressionService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(ExpressionService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<ExpressionService>(value),
    );
  }
}

String _$expressionServiceHash() => r'8226019ce7e8c5796019c1989c3e2cbb541cc667';
