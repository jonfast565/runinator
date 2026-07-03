// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'workflows_service.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(WorkflowsNotifier)
final workflowsProvider = WorkflowsNotifierProvider._();

final class WorkflowsNotifierProvider
    extends $NotifierProvider<WorkflowsNotifier, WorkflowServicesState> {
  WorkflowsNotifierProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'workflowsProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$workflowsNotifierHash();

  @$internal
  @override
  WorkflowsNotifier create() => WorkflowsNotifier();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(WorkflowServicesState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<WorkflowServicesState>(value),
    );
  }
}

String _$workflowsNotifierHash() => r'd0d1836ef8c07b5b8946f3d95c8dc37b7fa5db6c';

abstract class _$WorkflowsNotifier extends $Notifier<WorkflowServicesState> {
  WorkflowServicesState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<WorkflowServicesState, WorkflowServicesState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<WorkflowServicesState, WorkflowServicesState>,
              WorkflowServicesState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}
