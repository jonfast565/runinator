// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'node_pools_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(nodePoolsService)
final nodePoolsServiceProvider = NodePoolsServiceProvider._();

final class NodePoolsServiceProvider
    extends
        $FunctionalProvider<
          NodePoolsService,
          NodePoolsService,
          NodePoolsService
        >
    with $Provider<NodePoolsService> {
  NodePoolsServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'nodePoolsServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$nodePoolsServiceHash();

  @$internal
  @override
  $ProviderElement<NodePoolsService> $createElement($ProviderPointer pointer) =>
      $ProviderElement(pointer);

  @override
  NodePoolsService create(Ref ref) {
    return nodePoolsService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(NodePoolsService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<NodePoolsService>(value),
    );
  }
}

String _$nodePoolsServiceHash() => r'aeed8ae7817a011bbfbfaa6429fb7f68d1d80da9';
