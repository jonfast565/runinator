// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'replica_samples_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(replicaSamplesService)
final replicaSamplesServiceProvider = ReplicaSamplesServiceProvider._();

final class ReplicaSamplesServiceProvider
    extends
        $FunctionalProvider<
          ReplicaSamplesService,
          ReplicaSamplesService,
          ReplicaSamplesService
        >
    with $Provider<ReplicaSamplesService> {
  ReplicaSamplesServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'replicaSamplesServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$replicaSamplesServiceHash();

  @$internal
  @override
  $ProviderElement<ReplicaSamplesService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  ReplicaSamplesService create(Ref ref) {
    return replicaSamplesService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(ReplicaSamplesService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<ReplicaSamplesService>(value),
    );
  }
}

String _$replicaSamplesServiceHash() =>
    r'3720ecd528ef2e4eef17087a4968531d86d374ee';
