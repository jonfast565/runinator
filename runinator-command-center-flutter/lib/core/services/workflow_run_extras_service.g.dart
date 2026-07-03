// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'workflow_run_extras_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(workflowRunExtrasService)
final workflowRunExtrasServiceProvider = WorkflowRunExtrasServiceProvider._();

final class WorkflowRunExtrasServiceProvider
    extends
        $FunctionalProvider<
          WorkflowRunExtrasService,
          WorkflowRunExtrasService,
          WorkflowRunExtrasService
        >
    with $Provider<WorkflowRunExtrasService> {
  WorkflowRunExtrasServiceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'workflowRunExtrasServiceProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$workflowRunExtrasServiceHash();

  @$internal
  @override
  $ProviderElement<WorkflowRunExtrasService> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  WorkflowRunExtrasService create(Ref ref) {
    return workflowRunExtrasService(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(WorkflowRunExtrasService value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<WorkflowRunExtrasService>(value),
    );
  }
}

String _$workflowRunExtrasServiceHash() =>
    r'9aa1312dba5a0161b5bddfe731cfbbe3fb87f880';
