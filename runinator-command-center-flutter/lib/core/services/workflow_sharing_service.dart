// port of core/services/workflow-sharing.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/json.dart';
import '../domain/models/index.dart';
import 'app_service.dart';

part 'workflow_sharing_service.g.dart';

class WorkflowSharingService {
  const WorkflowSharingService(this._app);

  final AppNotifier _app;

  Future<List<JsonRecord>> listGrants(String workflowId) =>
      _app.runOperation('Loading workflow grants', () => api.listWorkflowGrants(workflowId));

  Future<JsonRecord> createGrant(
    String workflowId,
    PrincipalType principalType,
    String principalId,
    PermissionLevel permission,
  ) =>
      _app.runOperation(
        'Granting workflow access',
        () => api.createWorkflowGrant(workflowId, principalType, principalId, permission),
      );

  Future<TaskResponse> revokeGrant(String workflowId, String grantId) =>
      _app.runOperation('Revoking workflow access', () => api.revokeWorkflowGrant(workflowId, grantId));

  Future<WorkflowDefinition> setOwner(String workflowId, String? orgId) =>
      _app.runOperation('Updating workflow owner', () => api.setWorkflowOwner(workflowId, orgId));
}

@riverpod
WorkflowSharingService workflowSharingService(Ref ref) => WorkflowSharingService(ref.watch(appProvider.notifier));
