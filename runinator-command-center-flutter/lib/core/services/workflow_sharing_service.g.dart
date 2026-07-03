// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'workflow_sharing_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(workflowSharingService)
final workflowSharingServiceProvider = WorkflowSharingServiceProvider._();

final class WorkflowSharingServiceProvider
    extends
        $FunctionalProvider<
          WorkflowSharingService,
          WorkflowSharingService,
          WorkflowSharingService
        >
    with $Provider<WorkflowSharingService> {
  WorkflowSharingServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'workflowSharingServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$workflowSharingServiceHash();

  @$internal
  @override
  $ProviderElement<WorkflowSharingService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  WorkflowSharingService create(Ref ref) {
    return workflowSharingService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(WorkflowSharingService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<WorkflowSharingService>(value),
    );
  }
}

String _$workflowSharingServiceHash() =>
    r'8b29a061fbf2aa410a359b8477887395c57e3bbb';
