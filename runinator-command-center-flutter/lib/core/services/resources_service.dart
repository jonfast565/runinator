// port of core/services/resources.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/json.dart';
import '../domain/models/index.dart';
import '../navigation/app_tab.dart';
import '../utils/approvals.dart';
import '../utils/resources.dart';
import '../utils/values.dart';
import '../workflow/editor_defaults.dart' show boundedIndex;
import 'app_service.dart';
import 'gates_service.dart' show ConfirmContext;

part 'resources_service.g.dart';

final List<ResourceEndpoint> resourceEndpoints = [
  const ResourceEndpoint(label: 'External Items', endpoint: 'external_items'),
  const ResourceEndpoint(label: 'Approvals', endpoint: 'approvals'),
  const ResourceEndpoint(label: 'Events', endpoint: 'automation_events'),
];

class ResourcesState {
  const ResourcesState({
    required this.selectedResourceEndpoint,
    required this.resourceRecords,
    this.selectedResourceRecord,
    required this.hideResolved,
  });

  final String selectedResourceEndpoint;
  final List<JsonRecord> resourceRecords;
  final JsonRecord? selectedResourceRecord;
  final bool hideResolved;

  ResourcesState copyWith({
    String? selectedResourceEndpoint,
    List<JsonRecord>? resourceRecords,
    Object? selectedResourceRecord = _unset,
    bool? hideResolved,
  }) =>
      ResourcesState(
        selectedResourceEndpoint: selectedResourceEndpoint ?? this.selectedResourceEndpoint,
        resourceRecords: resourceRecords ?? this.resourceRecords,
        selectedResourceRecord:
            identical(selectedResourceRecord, _unset) ? this.selectedResourceRecord : selectedResourceRecord as JsonRecord?,
        hideResolved: hideResolved ?? this.hideResolved,
      );
}

const Object _unset = Object();

bool isResolved(JsonRecord? record) {
  if (record == null) {
    return false;
  }

  if (nonEmptyString(record['resolved_at']) != null) {
    return true;
  }

  final status = displayValue(record['status']).toLowerCase();
  return ['approved', 'rejected', 'resolved', 'cancelled', 'canceled', 'expired'].contains(status);
}

@riverpod
class ResourcesNotifier extends _$ResourcesNotifier {
  @override
  ResourcesState build() => const ResourcesState(
        selectedResourceEndpoint: 'external_items',
        resourceRecords: [],
        selectedResourceRecord: null,
        hideResolved: false,
      );

  String recordType(JsonRecord record) => genericRecordType(record, state.selectedResourceEndpoint);

  String recordSummary(JsonRecord record) => genericRecordSummary(record);

  List<JsonRecord> filteredResourceRecords() {
    final query = ref.read(appProvider.notifier).normalizedSearch;
    var records = state.resourceRecords;

    if (state.hideResolved && state.selectedResourceEndpoint == 'approvals') {
      records = records.where((record) => !isResolved(record)).toList();
    }

    if (query.isEmpty) {
      return records;
    }

    return records
        .where((record) => [
              record['id'],
              record['provider'],
              recordType(record),
              record['status'],
              recordSummary(record),
              record['external_id'],
              record['key'],
              record['url'],
            ].where((value) => value != null).any((value) => displayValue(value).toLowerCase().contains(query)))
        .toList();
  }

  bool canResolveApproval() =>
      state.selectedResourceEndpoint == 'approvals' &&
      nonEmptyString(state.selectedResourceRecord?['id']) != null &&
      !isResolved(state.selectedResourceRecord);

  bool canDeleteSelected() =>
      state.selectedResourceEndpoint == 'automation_events' && nonEmptyString(state.selectedResourceRecord?['id']) != null;

  void setHideResolved(bool value) {
    state = state.copyWith(hideResolved: value);
  }

  void setSelectedResourceEndpoint(String endpoint) {
    state = state.copyWith(selectedResourceEndpoint: endpoint);
  }

  void setSelectedResourceRecord(JsonRecord? record) {
    state = state.copyWith(selectedResourceRecord: record);
  }

  void setResourceRecords(List<JsonRecord> records) {
    state = state.copyWith(resourceRecords: records);
  }

  Future<void> refreshResources() async {
    final app = ref.read(appProvider.notifier);
    final endpoint = state.selectedResourceEndpoint;
    List<JsonRecord> records;
    try {
      records = await app.runOperation('Refreshing resources', () => api.fetchResourceRecords(endpoint));
    } catch (_) {
      records = [];
    }
    state = state.copyWith(resourceRecords: records, selectedResourceRecord: records.isNotEmpty ? records.first : null);
  }

  Future<void> refreshResourcesFor(String endpoint) async {
    if (state.selectedResourceEndpoint != endpoint) {
      state = state.copyWith(selectedResourceEndpoint: endpoint, selectedResourceRecord: null);
    }

    await refreshResources();
  }

  void clearResources() {
    state = state.copyWith(resourceRecords: const [], selectedResourceRecord: null);
  }

  Future<void> handleApprovalAction(String approvalId, ApprovalAction action) async {
    final app = ref.read(appProvider.notifier);
    final response = await app.runOperation(
      action == ApprovalAction.approve ? 'Approving approval' : 'Rejecting approval',
      () => action == ApprovalAction.approve ? api.approveApproval(approvalId) : api.rejectApproval(approvalId),
    );
    app.setStatus(
      response.message.isNotEmpty
          ? response.message
          : 'Approval ${action == ApprovalAction.approve ? 'approved' : 'rejected'}',
    );
    await refreshResources();
  }

  Future<void> resolveApproval(ApprovalAction action) async {
    final app = ref.read(appProvider.notifier);

    if (!canResolveApproval()) {
      app.setError('No approval selected');
      return;
    }

    final approvalId = nonEmptyString(state.selectedResourceRecord?['id']);

    if (approvalId == null) {
      app.setError('No approval selected');
      return;
    }

    await handleApprovalAction(approvalId, action);
  }

  Future<void> resolveWorkflowApproval(
    String workflowRunId,
    String nodeId,
    WorkflowNodeRun nodeRun,
    ApprovalAction action,
  ) async {
    final approvalId = await findWorkflowApprovalId(workflowRunId, nodeId, nodeRun);

    if (approvalId == null) {
      return;
    }

    await handleApprovalAction(approvalId, action);
  }

  Future<String?> findWorkflowApprovalId(String workflowRunId, String nodeId, WorkflowNodeRun nodeRun) async {
    final app = ref.read(appProvider.notifier);
    final stateApprovalId = approvalIdFromNodeRun(nodeRun);

    if (stateApprovalId != null) {
      return stateApprovalId;
    }

    final approvals = await app.runOperation(
      'Loading workflow approvals',
      () => api.fetchResourceRecords('approvals?workflow_run_id=$workflowRunId'),
    );
    final approval = selectWorkflowApprovalRecord(approvals, workflowRunId, nodeId);
    final approvalId = nonEmptyString(approval?['id']);

    if (approvalId != null) {
      return approvalId;
    }

    app.setError('No approval found for workflow node $nodeId');
    return null;
  }

  Future<void> deleteSelectedEvent() async {
    final app = ref.read(appProvider.notifier);
    final id = nonEmptyString(state.selectedResourceRecord?['id']);

    if (id == null) {
      app.setError('No record selected');
      return;
    }

    if (state.selectedResourceEndpoint != 'automation_events') {
      return;
    }

    try {
      await app.runOperation('Deleting event', () => api.deleteAutomationEvent(id));
    } catch (error) {
      app.setError(error.toString());
      return;
    }

    state = state.copyWith(
      resourceRecords: state.resourceRecords.where((record) => record['id'] != id).toList(),
      selectedResourceRecord: state.resourceRecords.where((record) => record['id'] != id).isNotEmpty
          ? state.resourceRecords.where((record) => record['id'] != id).first
          : null,
    );
    await refreshResources();
  }

  Future<void> deleteSelected(ConfirmContext confirm) async {
    final app = ref.read(appProvider.notifier);
    final id = nonEmptyString(state.selectedResourceRecord?['id']);

    if (id == null) {
      app.setError('No record selected');
      return;
    }

    if (state.selectedResourceEndpoint != 'automation_events') {
      return;
    }

    if (!confirm.confirm('Delete this event record?')) {
      return;
    }

    try {
      await app.runOperation('Deleting event', () => api.deleteAutomationEvent(id));
    } catch (error) {
      app.setError(error.toString());
    }

    state = state.copyWith(
      resourceRecords: state.resourceRecords.where((record) => record['id'] != id).toList(),
      selectedResourceRecord: state.resourceRecords.where((record) => record['id'] != id).isNotEmpty
          ? state.resourceRecords.where((record) => record['id'] != id).first
          : null,
    );
    await refreshResources();
  }

  void moveResourceSelection(int delta) {
    final list = filteredResourceRecords();

    if (list.isEmpty) {
      return;
    }

    final current = list.indexWhere((record) => identical(record, state.selectedResourceRecord));
    state = state.copyWith(selectedResourceRecord: list[boundedIndex(current, delta, list.length)]);
  }
}
