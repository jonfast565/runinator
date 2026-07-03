// port of core/api/commandCenterApi.ts.
//
// thin typed facade over command()/http_runtime's REGISTRY. dev-pack file access
// (inspectDevPack/readDevPackFile/writeDevPackFile/applyDevPack), artifact
// upload/download-to-path, and the embedded local worker functions are Tauri
// desktop-only in the source and are skipped here per the desktop-out-of-scope
// decision (they always throw/are unreachable in a web-only build).

import '../domain/json.dart';
import '../domain/models/index.dart';
import 'command_runtime.dart';
import 'http_runtime.dart' show setHttpAuthToken;

/// referenced by core/platform/types.dart's ArtifactTransport; the desktop-only
/// upload/download-to-path functions themselves are skipped (see file header).
class ArtifactUploadRequest {
  const ArtifactUploadRequest({required this.runId, this.workflowNodeRunId});

  final String runId;
  final String? workflowNodeRunId;

  Map<String, Object?> toJson() => {'run_id': runId, 'workflow_node_run_id': workflowNodeRunId};
}

class WorkflowWdlSaveRequest {
  const WorkflowWdlSaveRequest({
    required this.source,
    required this.enabled,
    this.workflowId,
    this.triggers,
    this.ui,
  });

  final String source;
  final bool enabled;
  final String? workflowId;
  final List<WorkflowTrigger>? triggers;
  final JsonRecord? ui;

  Map<String, Object?> toJson() => {
        'source': source,
        'enabled': enabled,
        'workflow_id': workflowId,
        'triggers': triggers?.map((t) => t.toJson()).toList(),
        'ui': ui,
      };
}

class AuthConfigResponse {
  const AuthConfigResponse({required this.enabled});

  factory AuthConfigResponse.fromJson(Map<String, Object?> json) =>
      AuthConfigResponse(enabled: json['enabled'] as bool);

  final bool enabled;
}

class LoginResult {
  const LoginResult({
    required this.accessToken,
    required this.refreshToken,
    required this.expiresIn,
    required this.user,
  });

  factory LoginResult.fromJson(Map<String, Object?> json) => LoginResult(
        accessToken: json['access_token'] as String,
        refreshToken: json['refresh_token'] as String,
        expiresIn: (json['expires_in'] as num).toInt(),
        user: asJsonRecord(json['user']),
      );

  final String accessToken;
  final String refreshToken;
  final int expiresIn;
  final JsonRecord user;
}

Future<AuthConfigResponse> fetchAuthConfig() async =>
    AuthConfigResponse.fromJson((await command('auth_config')) as Map<String, Object?>);

Future<JsonRecord> fetchAuthMe() async => asJsonRecord(await command('auth_me'));

Future<LoginResult> login(String username, String password) async => LoginResult.fromJson(
      (await command('login', {'username': username, 'password': password})) as Map<String, Object?>,
    );

Future<LoginResult> refreshSession(String refreshToken) async => LoginResult.fromJson(
      (await command('refresh_session', {'refreshToken': refreshToken})) as Map<String, Object?>,
    );

Future<TaskResponse> logout(String refreshToken) async => TaskResponse.fromJson(
      (await command('logout', {'refreshToken': refreshToken})) as Map<String, Object?>,
    );

/// push the access token to the web fetch layer (the only runtime in this port).
Future<void> setAccessToken(String? token) async {
  setHttpAuthToken(token);
}

Future<List<JsonRecord>> listWorkflowGrants(String workflowId) async =>
    ((await command('list_workflow_grants', {'workflowId': workflowId})) as List)
        .map((e) => asJsonRecord(e))
        .toList();

Future<JsonRecord> createWorkflowGrant(
  String workflowId,
  PrincipalType principalType,
  String principalId,
  PermissionLevel permission,
) async =>
    asJsonRecord(await command('create_workflow_grant', {
      'workflowId': workflowId,
      'principalType': principalType.wire,
      'principalId': principalId,
      'permission': permission.wire,
    }));

Future<TaskResponse> revokeWorkflowGrant(String workflowId, String grantId) async =>
    TaskResponse.fromJson(
      (await command('revoke_workflow_grant', {'workflowId': workflowId, 'grantId': grantId}))
          as Map<String, Object?>,
    );

class CreateUserInput {
  const CreateUserInput({required this.username, required this.password, this.email, this.isAdmin});

  final String username;
  final String password;
  final String? email;
  final bool? isAdmin;

  Map<String, Object?> toJson() =>
      {'username': username, 'password': password, 'email': email, 'is_admin': isAdmin};
}

class UpdateUserInput {
  const UpdateUserInput({this.email, this.password, this.isAdmin, this.disabled});

  final String? email;
  final String? password;
  final bool? isAdmin;
  final bool? disabled;

  Map<String, Object?> toJson() =>
      {'email': email, 'password': password, 'is_admin': isAdmin, 'disabled': disabled};
}

class CreateApiKeyInput {
  const CreateApiKeyInput({required this.name, this.userId, this.isService, this.expiresAt});

  final String name;
  final String? userId;
  final bool? isService;
  final String? expiresAt;

  Map<String, Object?> toJson() =>
      {'name': name, 'user_id': userId, 'is_service': isService, 'expires_at': expiresAt};
}

class UpdateApiKeyInput {
  const UpdateApiKeyInput({this.name, this.expiresAt, this.disabled});

  final String? name;
  final String? expiresAt;
  final bool? disabled;

  Map<String, Object?> toJson() => {'name': name, 'expires_at': expiresAt, 'disabled': disabled};
}

Future<List<User>> listUsers() async =>
    ((await command('list_users')) as List).map((e) => User.fromJson(e as Map<String, Object?>)).toList();

Future<User> createUser(CreateUserInput request) async => User.fromJson(
      (await command('create_user', {'request': request.toJson()})) as Map<String, Object?>,
    );

Future<User> updateUser(String userId, UpdateUserInput request) async => User.fromJson(
      (await command('update_user', {'userId': userId, 'request': request.toJson()}))
          as Map<String, Object?>,
    );

Future<TaskResponse> deleteUser(String userId) async => TaskResponse.fromJson(
      (await command('delete_user', {'userId': userId})) as Map<String, Object?>,
    );

Future<List<Team>> listTeams() async =>
    ((await command('list_teams')) as List).map((e) => Team.fromJson(e as Map<String, Object?>)).toList();

Future<Team> createTeam(String name) async =>
    Team.fromJson((await command('create_team', {'name': name})) as Map<String, Object?>);

Future<Team> updateTeam(String teamId, String name) async => Team.fromJson(
      (await command('update_team', {'teamId': teamId, 'name': name})) as Map<String, Object?>,
    );

Future<TaskResponse> deleteTeam(String teamId) async => TaskResponse.fromJson(
      (await command('delete_team', {'teamId': teamId})) as Map<String, Object?>,
    );

Future<List<User>> listTeamMembers(String teamId) async =>
    ((await command('list_team_members', {'teamId': teamId})) as List)
        .map((e) => User.fromJson(e as Map<String, Object?>))
        .toList();

Future<List<Team>> listUserTeams(String userId) async =>
    ((await command('list_user_teams', {'userId': userId})) as List)
        .map((e) => Team.fromJson(e as Map<String, Object?>))
        .toList();

Future<TaskResponse> addTeamMember(String teamId, String userId) async => TaskResponse.fromJson(
      (await command('add_team_member', {'teamId': teamId, 'userId': userId})) as Map<String, Object?>,
    );

Future<TaskResponse> removeTeamMember(String teamId, String userId) async => TaskResponse.fromJson(
      (await command('remove_team_member', {'teamId': teamId, 'userId': userId}))
          as Map<String, Object?>,
    );

Future<List<ApiKey>> listApiKeys() async => ((await command('list_api_keys')) as List)
    .map((e) => ApiKey.fromJson(e as Map<String, Object?>))
    .toList();

Future<CreateApiKeyResponse> createApiKey(CreateApiKeyInput request) async =>
    CreateApiKeyResponse.fromJson(
      (await command('create_api_key', {'request': request.toJson()})) as Map<String, Object?>,
    );

Future<ApiKey> updateApiKey(String keyId, UpdateApiKeyInput request) async => ApiKey.fromJson(
      (await command('update_api_key', {'keyId': keyId, 'request': request.toJson()}))
          as Map<String, Object?>,
    );

Future<TaskResponse> revokeApiKey(String keyId) async => TaskResponse.fromJson(
      (await command('revoke_api_key', {'keyId': keyId})) as Map<String, Object?>,
    );

Future<List<JsonRecord>> listDeadLetters({String? channel, int? limit}) async =>
    ((await command('list_dead_letters', {'channel': channel, 'limit': limit})) as List)
        .map((e) => asJsonRecord(e))
        .toList();

Future<List<JsonRecord>> listAuditLog({String? actorId, String? action, int? limit}) async =>
    ((await command('list_audit_log', {'actorId': actorId, 'action': action, 'limit': limit}))
            as List)
        .map((e) => asJsonRecord(e))
        .toList();

Future<CreateApiKeyResponse> rotateApiKey(String keyId) async => CreateApiKeyResponse.fromJson(
      (await command('rotate_api_key', {'keyId': keyId})) as Map<String, Object?>,
    );

Future<Grant> grantWorkflowAccess(
  String workflowId,
  PrincipalType principalType,
  String principalId,
  PermissionLevel permission,
) async =>
    Grant.fromJson((await command('create_workflow_grant', {
      'workflowId': workflowId,
      'principalType': principalType.wire,
      'principalId': principalId,
      'permission': permission.wire,
    })) as Map<String, Object?>);

Future<ServiceStatus> getServiceStatus() async =>
    ServiceStatus.fromJson((await command('get_service_status')) as Map<String, Object?>);

Future<void> startServiceDiscovery() async => command('start_service_discovery');

Future<List<RunChunk>> fetchRunChunks(String runId) async =>
    ((await command('fetch_run_chunks', {'runId': runId})) as List)
        .map((e) => RunChunk.fromJson(e as Map<String, Object?>))
        .toList();

Future<List<RunArtifact>> fetchRunArtifacts(String runId) async =>
    ((await command('fetch_run_artifacts', {'runId': runId})) as List)
        .map((e) => RunArtifact.fromJson(e as Map<String, Object?>))
        .toList();

Future<List<RunChunk>> fetchWorkflowNodeRunChunks(String nodeRunId) async =>
    ((await command('fetch_workflow_node_run_chunks', {'nodeRunId': nodeRunId})) as List)
        .map((e) => RunChunk.fromJson(e as Map<String, Object?>))
        .toList();

Future<List<RunArtifact>> fetchWorkflowNodeRunArtifacts(String nodeRunId) async =>
    ((await command('fetch_workflow_node_run_artifacts', {'nodeRunId': nodeRunId})) as List)
        .map((e) => RunArtifact.fromJson(e as Map<String, Object?>))
        .toList();

Future<List<WorkflowRunArtifact>> fetchWorkflowRunArtifacts(String workflowRunId) async =>
    ((await command('fetch_workflow_run_artifacts', {'workflowRunId': workflowRunId})) as List)
        .map((e) => WorkflowRunArtifact.fromJson(e as Map<String, Object?>))
        .toList();

Future<List<WorkflowDefinition>> fetchWorkflows() async =>
    ((await command('fetch_workflows')) as List)
        .map((e) => WorkflowDefinition.fromJson(e as Map<String, Object?>))
        .toList();

Future<WorkflowDefinition> saveWorkflow(WorkflowDefinition workflow) async => WorkflowDefinition.fromJson(
      (await command('save_workflow', {'workflow': workflow.toJson()})) as Map<String, Object?>,
    );

Future<WorkflowBundle> saveWorkflowBundle(WorkflowBundle request) async => WorkflowBundle.fromJson(
      (await command('save_workflow_bundle', {'request': request.toJson()})) as Map<String, Object?>,
    );

Future<WorkflowBundle> saveWorkflowWdl(WorkflowWdlSaveRequest request) async => WorkflowBundle.fromJson(
      (await command('save_workflow_wdl', {'request': request.toJson()})) as Map<String, Object?>,
    );

Future<WorkflowDefinition> compileWdl(String source, bool enabled) async => WorkflowDefinition.fromJson(
      (await command('compile_wdl', {'source': source, 'enabled': enabled})) as Map<String, Object?>,
    );

Future<List<WdlDiagnostic>> analyzeWdl(String source, [String? sourcePath]) async =>
    ((await command('analyze_wdl', {'source': source, 'sourcePath': sourcePath})) as List)
        .map((e) => WdlDiagnostic.fromJson(e as Map<String, Object?>))
        .toList();

Future<WdlCompletionResponse> completeWdl(WdlCompletionRequest request) async =>
    WdlCompletionResponse.fromJson(
      (await command('complete_wdl', {'request': request.toJson()})) as Map<String, Object?>,
    );

Future<WdlHoverResponse?> hoverWdl(WdlHoverRequest request) async {
  final raw = await command('hover_wdl', {'request': request.toJson()});
  return raw != null ? WdlHoverResponse.fromJson(raw as Map<String, Object?>) : null;
}

Future<String> formatWdl(String source) async =>
    (await command('format_wdl', {'source': source})) as String;

Future<String> decompileToWdl(WorkflowDefinition workflow) async =>
    (await command('decompile_to_wdl', {'workflow': workflow.toJson()})) as String;

Future<Object?> evaluateExpression(Object? expression, Object? context) async =>
    command('evaluate_expression', {'expression': expression, 'context': context});

Future<TaskResponse> deleteWorkflow(String workflowId) async => TaskResponse.fromJson(
      (await command('delete_workflow', {'workflowId': workflowId})) as Map<String, Object?>,
    );

Future<WorkflowDefinition> duplicateWorkflow(String workflowId, [String bump = 'minor']) async =>
    WorkflowDefinition.fromJson(
      (await command('duplicate_workflow', {'workflowId': workflowId, 'bump': bump}))
          as Map<String, Object?>,
    );

Future<List<WorkflowTrigger>> fetchWorkflowTriggers(String workflowId) async =>
    ((await command('fetch_workflow_triggers', {'workflowId': workflowId})) as List)
        .map((e) => WorkflowTrigger.fromJson(e as Map<String, Object?>))
        .toList();

Future<WorkflowTrigger> saveWorkflowTrigger(WorkflowTrigger trigger, bool creating) async =>
    WorkflowTrigger.fromJson(
      (await command('save_workflow_trigger', {'trigger': trigger.toJson(), 'creating': creating}))
          as Map<String, Object?>,
    );

Future<TaskResponse> deleteWorkflowTrigger(String triggerId) async => TaskResponse.fromJson(
      (await command('delete_workflow_trigger', {'triggerId': triggerId})) as Map<String, Object?>,
    );

Future<WorkflowRunCreated> createWorkflowRun(
  String workflowId, {
  bool debug = false,
  Object? parameters,
}) async =>
    WorkflowRunCreated.fromJson((await command('create_workflow_run', {
      'workflowId': workflowId,
      'debug': debug,
      'parameters': parameters ?? <String, Object?>{},
    })) as Map<String, Object?>);

Future<List<RunSummary>> fetchWorkflowRuns([String? workflowId]) async =>
    ((await command('fetch_workflow_runs', {'workflowId': workflowId})) as List)
        .map((e) => RunSummary.fromJson(e as Map<String, Object?>))
        .toList();

Future<WorkflowRunDetail> fetchWorkflowRun(String workflowRunId) async => WorkflowRunDetail.fromJson(
      (await command('fetch_workflow_run', {'workflowRunId': workflowRunId})) as Map<String, Object?>,
    );

Future<TaskResponse> stepWorkflowRun(String workflowRunId) async => TaskResponse.fromJson(
      (await command('step_workflow_run', {'workflowRunId': workflowRunId})) as Map<String, Object?>,
    );

Future<TaskResponse> continueWorkflowRun(String workflowRunId) async => TaskResponse.fromJson(
      (await command('continue_workflow_run', {'workflowRunId': workflowRunId}))
          as Map<String, Object?>,
    );

Future<TaskResponse> cancelWorkflowRun(String workflowRunId) async => TaskResponse.fromJson(
      (await command('cancel_workflow_run', {'workflowRunId': workflowRunId})) as Map<String, Object?>,
    );

Future<TaskResponse> pauseWorkflowRun(String workflowRunId) async => TaskResponse.fromJson(
      (await command('pause_workflow_run', {'workflowRunId': workflowRunId})) as Map<String, Object?>,
    );

Future<TaskResponse> resumeWorkflowRun(String workflowRunId) async => TaskResponse.fromJson(
      (await command('resume_workflow_run', {'workflowRunId': workflowRunId})) as Map<String, Object?>,
    );

class WorkflowDebugPatch {
  const WorkflowDebugPatch({this.breakpoints, this.mode, this.oneShotBreakpoint});

  final List<String>? breakpoints;
  final DebugMode? mode;
  final String? oneShotBreakpoint;

  Map<String, Object?> toJson() => {
        'breakpoints': breakpoints,
        'mode': mode?.wire,
        'one_shot_breakpoint': oneShotBreakpoint,
      };
}

Future<TaskResponse> patchWorkflowRunDebug(String workflowRunId, WorkflowDebugPatch patch) async =>
    TaskResponse.fromJson((await command(
      'patch_workflow_run_debug',
      {'workflowRunId': workflowRunId, 'patch': patch.toJson()},
    )) as Map<String, Object?>);

Future<TaskResponse> runToCursorWorkflowRun(String workflowRunId, String nodeId) async =>
    TaskResponse.fromJson((await command(
      'run_to_cursor_workflow_run',
      {'workflowRunId': workflowRunId, 'nodeId': nodeId},
    )) as Map<String, Object?>);

Future<TaskResponse> skipWorkflowNode(String workflowRunId, Object? outputJson, [String? message]) async =>
    TaskResponse.fromJson((await command('skip_workflow_node', {
      'workflowRunId': workflowRunId,
      'outputJson': outputJson,
      'message': message,
    })) as Map<String, Object?>);

Future<TaskResponse> resolveWorkflowInput(
  String nodeRunId,
  Object? outputJson, {
  String? resolvedBy,
  String? message,
}) async =>
    TaskResponse.fromJson((await command('resolve_workflow_input', {
      'nodeRunId': nodeRunId,
      'outputJson': outputJson,
      'resolvedBy': resolvedBy,
      'message': message,
    })) as Map<String, Object?>);

Future<TaskResponse> rerunWorkflowNode(String workflowRunId, Object? parameters) async =>
    TaskResponse.fromJson((await command(
      'rerun_workflow_node',
      {'workflowRunId': workflowRunId, 'parameters': parameters},
    )) as Map<String, Object?>);

Future<WorkflowRunCreated> replayWorkflowRun(String workflowRunId, {String? fromStepId}) async =>
    WorkflowRunCreated.fromJson((await command(
      'replay_workflow_run',
      {'workflowRunId': workflowRunId, 'fromStepId': fromStepId},
    )) as Map<String, Object?>);

Future<TaskResponse> renameWorkflowRun(String workflowRunId, String? name) async =>
    TaskResponse.fromJson((await command(
      'rename_workflow_run',
      {'workflowRunId': workflowRunId, 'name': name},
    )) as Map<String, Object?>);

Future<List<RunArtifact>> fetchAllArtifacts() async => ((await command('fetch_all_artifacts')) as List)
    .map((e) => RunArtifact.fromJson(e as Map<String, Object?>))
    .toList();

Future<List<Notification>> fetchNotifications({bool unreadOnly = false, int limit = 200}) async =>
    ((await command('fetch_notifications', {'unreadOnly': unreadOnly, 'limit': limit})) as List)
        .map((e) => Notification.fromJson(e as Map<String, Object?>))
        .toList();

Future<Notification> markNotificationRead(String notificationId) async => Notification.fromJson(
      (await command('mark_notification_read', {'notificationId': notificationId}))
          as Map<String, Object?>,
    );

Future<TaskResponse> markAllNotificationsRead() async => TaskResponse.fromJson(
      (await command('mark_all_notifications_read')) as Map<String, Object?>,
    );

Future<TaskResponse> deleteNotification(String notificationId) async => TaskResponse.fromJson(
      (await command('delete_notification', {'notificationId': notificationId}))
          as Map<String, Object?>,
    );

Future<TaskResponse> deleteArtifact(String artifactId) async => TaskResponse.fromJson(
      (await command('delete_artifact', {'artifactId': artifactId})) as Map<String, Object?>,
    );

Future<TaskResponse> deleteGate(String gateId) async => TaskResponse.fromJson(
      (await command('delete_gate', {'gateId': gateId})) as Map<String, Object?>,
    );

Future<TaskResponse> deleteAutomationEvent(String eventId) async => TaskResponse.fromJson(
      (await command('delete_automation_event', {'eventId': eventId})) as Map<String, Object?>,
    );

class ReplicaSample {
  const ReplicaSample({
    required this.replicaId,
    required this.sampledAt,
    required this.cpuPercent,
    required this.memPercent,
    required this.memUsedBytes,
    required this.memTotalBytes,
    this.loadOne,
    required this.processCpuPercent,
    required this.processMemBytes,
    required this.netRxBytesPerSec,
    required this.netTxBytesPerSec,
  });

  factory ReplicaSample.fromJson(Map<String, Object?> json) => ReplicaSample(
        replicaId: json['replica_id'] as String,
        sampledAt: json['sampled_at'] as String,
        cpuPercent: (json['cpu_percent'] as num).toDouble(),
        memPercent: (json['mem_percent'] as num).toDouble(),
        memUsedBytes: (json['mem_used_bytes'] as num).toInt(),
        memTotalBytes: (json['mem_total_bytes'] as num).toInt(),
        loadOne: (json['load_one'] as num?)?.toDouble(),
        processCpuPercent: (json['process_cpu_percent'] as num).toDouble(),
        processMemBytes: (json['process_mem_bytes'] as num).toInt(),
        netRxBytesPerSec: (json['net_rx_bytes_per_sec'] as num).toDouble(),
        netTxBytesPerSec: (json['net_tx_bytes_per_sec'] as num).toDouble(),
      );

  final String replicaId;
  final String sampledAt;
  final double cpuPercent;
  final double memPercent;
  final int memUsedBytes;
  final int memTotalBytes;
  final double? loadOne;
  final double processCpuPercent;
  final int processMemBytes;
  final double netRxBytesPerSec;
  final double netTxBytesPerSec;
}

class ReplicaSampleSeries {
  const ReplicaSampleSeries({required this.replicaId, required this.samples});

  factory ReplicaSampleSeries.fromJson(Map<String, Object?> json) => ReplicaSampleSeries(
        replicaId: json['replica_id'] as String,
        samples: (json['samples'] as List)
            .map((s) => ReplicaSample.fromJson(s as Map<String, Object?>))
            .toList(),
      );

  final String replicaId;
  final List<ReplicaSample> samples;
}

Future<ReplicaSampleSeries> fetchReplicaSamples(String replicaId, [int? sinceSeconds]) async =>
    ReplicaSampleSeries.fromJson((await command(
      'fetch_replica_samples',
      {'replicaId': replicaId, 'sinceSeconds': sinceSeconds},
    )) as Map<String, Object?>);

Future<WorkflowDefinition> setWorkflowOwner(String workflowId, String? orgId) async =>
    WorkflowDefinition.fromJson((await command(
      'set_workflow_owner',
      {'workflowId': workflowId, 'orgId': orgId},
    )) as Map<String, Object?>);

class SupervisorProcessSnapshot {
  const SupervisorProcessSnapshot({
    required this.name,
    required this.status,
    this.pid,
    required this.restarts,
    this.uptimeSeconds,
    this.lastExitCode,
    this.lastError,
    this.startedAt,
    required this.command,
    required this.cwd,
    required this.logFile,
  });

  factory SupervisorProcessSnapshot.fromJson(Map<String, Object?> json) => SupervisorProcessSnapshot(
        name: json['name'] as String,
        status: json['status'] as String,
        pid: (json['pid'] as num?)?.toInt(),
        restarts: (json['restarts'] as num).toInt(),
        uptimeSeconds: (json['uptime_seconds'] as num?)?.toInt(),
        lastExitCode: (json['last_exit_code'] as num?)?.toInt(),
        lastError: json['last_error'] as String?,
        startedAt: json['started_at'] as String?,
        command: json['command'] as String,
        cwd: json['cwd'] as String,
        logFile: json['log_file'] as String,
      );

  final String name;
  final String status;
  final int? pid;
  final int restarts;
  final int? uptimeSeconds;
  final int? lastExitCode;
  final String? lastError;
  final String? startedAt;
  final String command;
  final String cwd;
  final String logFile;
}

class SupervisorStatus {
  const SupervisorStatus({
    required this.configured,
    this.path,
    this.supervisorPid,
    this.configPath,
    this.startedAt,
    this.updatedAt,
    this.processes,
    this.staleSeconds,
    this.error,
  });

  factory SupervisorStatus.fromJson(Map<String, Object?> json) => SupervisorStatus(
        configured: json['configured'] as bool,
        path: json['path'] as String?,
        supervisorPid: (json['supervisor_pid'] as num?)?.toInt(),
        configPath: json['config_path'] as String?,
        startedAt: json['started_at'] as String?,
        updatedAt: json['updated_at'] as String?,
        processes: (json['processes'] as List?)
            ?.map((p) => SupervisorProcessSnapshot.fromJson(p as Map<String, Object?>))
            .toList(),
        staleSeconds: (json['stale_seconds'] as num?)?.toInt(),
        error: json['error'] as String?,
      );

  final bool configured;
  final String? path;
  final int? supervisorPid;
  final String? configPath;
  final String? startedAt;
  final String? updatedAt;
  final List<SupervisorProcessSnapshot>? processes;
  final int? staleSeconds;
  final String? error;
}

Future<SupervisorStatus> fetchSupervisorStatus() async =>
    SupervisorStatus.fromJson((await command('fetch_supervisor_status')) as Map<String, Object?>);

Future<List<JsonRecord>> fetchResourceRecords(String endpoint) async =>
    ((await command('fetch_resource_records', {'endpoint': endpoint})) as List)
        .map((e) => asJsonRecord(e))
        .toList();

Future<List<ProviderMetadata>> fetchProviders() async => ((await command('fetch_providers')) as List)
    .map((e) => ProviderMetadata.fromJson(e as Map<String, Object?>))
    .toList();

Future<ReplicaListResponse> fetchReplicas() async =>
    ReplicaListResponse.fromJson((await command('fetch_replicas')) as Map<String, Object?>);

// --- on-demand node provisioning (supervisor / kubernetes backends) ---

class NodeBackendInfo {
  const NodeBackendInfo({required this.backend, required this.kinds, required this.available});

  factory NodeBackendInfo.fromJson(Map<String, Object?> json) => NodeBackendInfo(
        backend: json['backend'] as String,
        kinds: (json['kinds'] as List).cast<String>(),
        available: json['available'] as bool,
      );

  final String backend;
  final List<String> kinds;
  final bool available;
}

class NodeBackendsResponse {
  const NodeBackendsResponse({required this.backends});

  factory NodeBackendsResponse.fromJson(Map<String, Object?> json) => NodeBackendsResponse(
        backends: (json['backends'] as List)
            .map((b) => NodeBackendInfo.fromJson(b as Map<String, Object?>))
            .toList(),
      );

  final List<NodeBackendInfo> backends;
}

class ProvisionedGroup {
  const ProvisionedGroup({
    required this.backend,
    required this.kind,
    required this.name,
    required this.desired,
    required this.available,
    required this.manageable,
  });

  factory ProvisionedGroup.fromJson(Map<String, Object?> json) => ProvisionedGroup(
        backend: json['backend'] as String,
        kind: json['kind'] as String,
        name: json['name'] as String,
        desired: (json['desired'] as num).toInt(),
        available: (json['available'] as num).toInt(),
        manageable: json['manageable'] as bool,
      );

  final String backend;
  final String kind;
  final String name;
  final int desired;
  final int available;
  final bool manageable;
}

class NodeSpec {
  const NodeSpec({this.labels, this.image, this.extraArgs, this.group});

  final Map<String, String>? labels;
  final String? image;
  final List<String>? extraArgs;
  final String? group;

  Map<String, Object?> toJson() =>
      {'labels': labels, 'image': image, 'extra_args': extraArgs, 'group': group};
}

class ScaleNodesRequest {
  const ScaleNodesRequest({required this.backend, required this.kind, required this.desired, this.spec});

  final String backend;
  final String kind;
  final int desired;
  final NodeSpec? spec;

  Map<String, Object?> toJson() =>
      {'backend': backend, 'kind': kind, 'desired': desired, 'spec': spec?.toJson()};
}

class StopNodeRequest {
  const StopNodeRequest({required this.backend, required this.nodeId});

  final String backend;
  final String nodeId;

  Map<String, Object?> toJson() => {'backend': backend, 'node_id': nodeId};
}

Future<NodeBackendsResponse> fetchNodeBackends() async =>
    NodeBackendsResponse.fromJson((await command('fetch_node_backends')) as Map<String, Object?>);

Future<List<ProvisionedGroup>> fetchNodes() async => ((await command('fetch_nodes')) as List)
    .map((e) => ProvisionedGroup.fromJson(e as Map<String, Object?>))
    .toList();

Future<ProvisionedGroup> scaleNodes(ScaleNodesRequest request) async => ProvisionedGroup.fromJson(
      (await command('scale_nodes', {'request': request.toJson()})) as Map<String, Object?>,
    );

Future<JsonRecord> stopNode(StopNodeRequest request) async =>
    asJsonRecord(await command('stop_node', {'request': request.toJson()}));

// --- organizations (tenants), membership, resource allocation, and billing ---

enum OrgRole {
  owner('owner'),
  admin('admin'),
  member('member');

  const OrgRole(this.wire);

  final String wire;

  static OrgRole fromJson(String value) =>
      OrgRole.values.firstWhere((role) => role.wire == value, orElse: () => OrgRole.member);
}

class Organization {
  const Organization({
    required this.id,
    required this.name,
    required this.slug,
    required this.disabled,
    required this.createdAt,
    required this.updatedAt,
  });

  factory Organization.fromJson(Map<String, Object?> json) => Organization(
        id: json['id'] as String,
        name: json['name'] as String,
        slug: json['slug'] as String,
        disabled: json['disabled'] as bool,
        createdAt: json['created_at'] as String,
        updatedAt: json['updated_at'] as String,
      );

  final String id;
  final String name;
  final String slug;
  final bool disabled;
  final String createdAt;
  final String updatedAt;
}

class OrgMembershipView {
  const OrgMembershipView({required this.org, required this.role});

  factory OrgMembershipView.fromJson(Map<String, Object?> json) => OrgMembershipView(
        org: Organization.fromJson(json['org'] as Map<String, Object?>),
        role: OrgRole.fromJson(json['role'] as String),
      );

  final Organization org;
  final OrgRole role;
}

class OrgMembership {
  const OrgMembership({
    required this.orgId,
    required this.userId,
    required this.role,
    required this.createdAt,
  });

  factory OrgMembership.fromJson(Map<String, Object?> json) => OrgMembership(
        orgId: json['org_id'] as String,
        userId: json['user_id'] as String,
        role: OrgRole.fromJson(json['role'] as String),
        createdAt: json['created_at'] as String,
      );

  final String orgId;
  final String userId;
  final OrgRole role;
  final String createdAt;
}

class OrgContextResponse {
  const OrgContextResponse({
    required this.accessToken,
    required this.expiresIn,
    required this.org,
    required this.role,
  });

  factory OrgContextResponse.fromJson(Map<String, Object?> json) => OrgContextResponse(
        accessToken: json['access_token'] as String,
        expiresIn: (json['expires_in'] as num).toInt(),
        org: Organization.fromJson(json['org'] as Map<String, Object?>),
        role: OrgRole.fromJson(json['role'] as String),
      );

  final String accessToken;
  final int expiresIn;
  final Organization org;
  final OrgRole role;
}

class OrgResourceGroup {
  const OrgResourceGroup({
    required this.orgId,
    required this.backend,
    required this.kind,
    required this.desired,
    required this.dedicated,
  });

  factory OrgResourceGroup.fromJson(Map<String, Object?> json) => OrgResourceGroup(
        orgId: json['org_id'] as String,
        backend: json['backend'] as String,
        kind: json['kind'] as String,
        desired: (json['desired'] as num).toInt(),
        dedicated: json['dedicated'] as bool,
      );

  final String orgId;
  final String backend;
  final String kind;
  final int desired;
  final bool dedicated;
}

class OrgNodesResponse {
  const OrgNodesResponse({required this.groups, required this.projectedMonthlyCents});

  factory OrgNodesResponse.fromJson(Map<String, Object?> json) => OrgNodesResponse(
        groups: (json['groups'] as List)
            .map((g) => OrgResourceGroup.fromJson(g as Map<String, Object?>))
            .toList(),
        projectedMonthlyCents: (json['projected_monthly_cents'] as num).toInt(),
      );

  final List<OrgResourceGroup> groups;
  final int projectedMonthlyCents;
}

class OrgQuota {
  const OrgQuota({required this.orgId, required this.maxNodesPerKind, required this.maxMonthlyCents});

  factory OrgQuota.fromJson(Map<String, Object?> json) => OrgQuota(
        orgId: json['org_id'] as String,
        maxNodesPerKind:
            (json['max_nodes_per_kind'] as Map).map((k, v) => MapEntry(k as String, (v as num).toInt())),
        maxMonthlyCents: (json['max_monthly_cents'] as num).toInt(),
      );

  final String orgId;
  final Map<String, int> maxNodesPerKind;
  final int maxMonthlyCents;
}

class OrgUsage {
  const OrgUsage({required this.orgId, this.since, required this.nodeHours, required this.accruedCents});

  factory OrgUsage.fromJson(Map<String, Object?> json) => OrgUsage(
        orgId: json['org_id'] as String,
        since: json['since'] as String?,
        nodeHours:
            (json['node_hours'] as Map).map((k, v) => MapEntry(k as String, (v as num).toDouble())),
        accruedCents: (json['accrued_cents'] as num).toInt(),
      );

  final String orgId;
  final String? since;
  final Map<String, double> nodeHours;
  final int accruedCents;
}

class RateEntry {
  const RateEntry({required this.backend, required this.kind, required this.hourlyCents});

  factory RateEntry.fromJson(Map<String, Object?> json) => RateEntry(
        backend: json['backend'] as String,
        kind: json['kind'] as String,
        hourlyCents: (json['hourly_cents'] as num).toInt(),
      );

  final String backend;
  final String kind;
  final int hourlyCents;
}

class RateCard {
  const RateCard({required this.entries});

  factory RateCard.fromJson(Map<String, Object?> json) => RateCard(
        entries:
            (json['entries'] as List).map((e) => RateEntry.fromJson(e as Map<String, Object?>)).toList(),
      );

  final List<RateEntry> entries;
}

class ScaleOrgNodesRequest {
  const ScaleOrgNodesRequest({required this.backend, required this.kind, required this.desired});

  final String backend;
  final String kind;
  final int desired;

  Map<String, Object?> toJson() => {'backend': backend, 'kind': kind, 'desired': desired};
}

Future<List<OrgMembershipView>> listMyOrgs() async => ((await command('list_my_orgs')) as List)
    .map((e) => OrgMembershipView.fromJson(e as Map<String, Object?>))
    .toList();

Future<List<Organization>> listOrgs() async => ((await command('list_orgs')) as List)
    .map((e) => Organization.fromJson(e as Map<String, Object?>))
    .toList();

Future<Organization> createOrg(String name) async =>
    Organization.fromJson((await command('create_org', {'name': name})) as Map<String, Object?>);

Future<OrgContextResponse> switchOrg(String orgId) async => OrgContextResponse.fromJson(
      (await command('switch_org', {'orgId': orgId})) as Map<String, Object?>,
    );

Future<List<OrgMembership>> listOrgMembers(String orgId) async =>
    ((await command('list_org_members', {'orgId': orgId})) as List)
        .map((e) => OrgMembership.fromJson(e as Map<String, Object?>))
        .toList();

Future<JsonRecord> addOrgMember(String orgId, String userId, OrgRole role) async => asJsonRecord(
      await command('add_org_member', {'orgId': orgId, 'userId': userId, 'role': role.wire}),
    );

Future<JsonRecord> updateOrgMember(String orgId, String userId, OrgRole role) async => asJsonRecord(
      await command('update_org_member', {'orgId': orgId, 'userId': userId, 'role': role.wire}),
    );

Future<JsonRecord> removeOrgMember(String orgId, String userId) async =>
    asJsonRecord(await command('remove_org_member', {'orgId': orgId, 'userId': userId}));

Future<RateCard> fetchRateCard() async =>
    RateCard.fromJson((await command('fetch_rate_card')) as Map<String, Object?>);

Future<OrgNodesResponse> fetchOrgNodes(String orgId) async => OrgNodesResponse.fromJson(
      (await command('fetch_org_nodes', {'orgId': orgId})) as Map<String, Object?>,
    );

Future<OrgResourceGroup> scaleOrgNodes(String orgId, ScaleOrgNodesRequest request) async =>
    OrgResourceGroup.fromJson((await command(
      'scale_org_nodes',
      {'orgId': orgId, 'request': request.toJson()},
    )) as Map<String, Object?>);

Future<OrgQuota> fetchOrgQuota(String orgId) async =>
    OrgQuota.fromJson((await command('fetch_org_quota', {'orgId': orgId})) as Map<String, Object?>);

Future<OrgUsage> fetchOrgUsage(String orgId) async =>
    OrgUsage.fromJson((await command('fetch_org_usage', {'orgId': orgId})) as Map<String, Object?>);

// --- credentials / config-and-secrets store ---

const String _foreignLanguageScope = 'foreign_languages';

Future<List<CredentialSummary>> fetchCredentials() async => ((await command('fetch_credentials')) as List)
    .map((e) => CredentialSummary.fromJson(e as Map<String, Object?>))
    .toList();

Future<CredentialDetail> fetchCredential(
  String scope,
  String name, [
  SettingKind kind = SettingKind.secret,
]) async =>
    CredentialDetail.fromJson((await command(
      'fetch_credential',
      {'scope': scope, 'name': name, 'kind': kind.wire},
    )) as Map<String, Object?>);

Future<TaskResponse> saveCredential(
  String scope,
  String name,
  Object? value, [
  SettingKind kind = SettingKind.secret,
  Object? schema,
]) async =>
    TaskResponse.fromJson((await command('save_credential', {
      'request': {'scope': scope, 'name': name, 'value': value, 'kind': kind.wire, 'schema': schema},
    })) as Map<String, Object?>);

Future<TaskResponse> deleteCredential(
  String scope,
  String name, [
  SettingKind kind = SettingKind.secret,
]) async =>
    TaskResponse.fromJson((await command(
      'delete_credential',
      {'scope': scope, 'name': name, 'kind': kind.wire},
    )) as Map<String, Object?>);

class ForeignLanguageRuntimeConfig {
  const ForeignLanguageRuntimeConfig({required this.image, required this.setupScript});

  factory ForeignLanguageRuntimeConfig.fromJson(Map<String, Object?> json) =>
      ForeignLanguageRuntimeConfig(
        image: json['image'] as String,
        setupScript: json['setup_script'] as String,
      );

  final String image;
  final String setupScript;

  Map<String, Object?> toJson() => {'image': image, 'setup_script': setupScript};
}

Future<CredentialDetail> fetchForeignLanguageRuntime(String language) async =>
    fetchCredential(_foreignLanguageScope, language, SettingKind.config);

Future<TaskResponse> saveForeignLanguageRuntime(String language, ForeignLanguageRuntimeConfig value) async =>
    saveCredential(_foreignLanguageScope, language, value.toJson(), SettingKind.config);

Future<TaskResponse> approveApproval(String approvalId) async => TaskResponse.fromJson(
      (await command('approve_approval', {'approvalId': approvalId})) as Map<String, Object?>,
    );

Future<TaskResponse> rejectApproval(String approvalId) async => TaskResponse.fromJson(
      (await command('reject_approval', {'approvalId': approvalId})) as Map<String, Object?>,
    );

Future<List<GateRecord>> fetchGates({String? workflowRunId, String? status}) async {
  final query = <String, String>{};

  final trimmedRunId = workflowRunId?.trim();
  if (trimmedRunId != null && trimmedRunId.isNotEmpty) {
    query['workflow_run_id'] = trimmedRunId;
  }

  final trimmedStatus = status?.trim();
  if (trimmedStatus != null && trimmedStatus.isNotEmpty) {
    query['status'] = trimmedStatus;
  }

  final suffix = query.isEmpty ? '' : '?${Uri(queryParameters: query).query}';

  return ((await command('fetch_resource_records', {'endpoint': 'gates$suffix'})) as List)
      .map((e) => GateRecord.fromJson(e as Map<String, Object?>))
      .toList();
}

Future<TaskResponse> openGate(String gateId, [String? reason]) async => TaskResponse.fromJson(
      (await command('open_gate', {'gateId': gateId, 'reason': reason})) as Map<String, Object?>,
    );

Future<TaskResponse> closeGate(String gateId, [String? reason]) async => TaskResponse.fromJson(
      (await command('close_gate', {'gateId': gateId, 'reason': reason})) as Map<String, Object?>,
    );

Future<TaskResponse> deliverSignal(String workflowRunId, String name, [Object? payload = const {}]) async =>
    TaskResponse.fromJson((await command('deliver_signal', {
      'workflowRunId': workflowRunId,
      'name': name,
      'payload': payload,
    })) as Map<String, Object?>);
