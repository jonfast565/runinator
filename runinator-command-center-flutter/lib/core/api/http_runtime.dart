// port of core/api/httpRuntime.ts.
//
// web-mode runtime: translates command names + args into HTTP requests against
// runinator-ws. this is the ONLY CommandRuntime implementation in this port,
// since Tauri desktop support is out of scope for a web target — there is no
// isTauri() branch anywhere in this file, unlike the ts source.
//
// path conventions: all paths are relative to apiBaseUrl(). in production the
// SPA is served behind a reverse proxy that forwards "/api/*" to runinator-ws,
// so apiBaseUrl() returns "/api" by default. pass --dart-define=RUNINATOR_WS_URL=...
// to override (mirrors the ts source's VITE_RUNINATOR_WS_URL).

import 'package:dio/dio.dart';

import '../utils/values.dart';

enum HttpMethod {
  get('GET'),
  post('POST'),
  patch('PATCH'),
  delete('DELETE');

  const HttpMethod(this.wire);

  final String wire;
}

typedef CommandArgs = Map<String, Object?>?;

class HttpDescriptor {
  const HttpDescriptor({
    required this.method,
    required this.path,
    this.body,
    this.headers,
    this.transform,
    this.accept404 = false,
  });

  final HttpMethod Function(CommandArgs args) method;
  final String Function(CommandArgs args) path;
  final Object? Function(CommandArgs args)? body;
  final Map<String, String> Function(CommandArgs args)? headers;
  final Object? Function(Object? raw)? transform;
  final bool accept404;
}

const String _workflowJsonImportRiskHeader = 'x-runinator-json-workflow-risk';
const String _workflowJsonImportRiskAck = 'system-breakage-possible';

// access token presented as `Authorization: Bearer …` in web mode; also appended to WS urls.
String? _authToken;

void setHttpAuthToken(String? token) {
  _authToken = (token != null && token.isNotEmpty) ? token : null;
}

String? httpAuthToken() => _authToken;

Map<String, String> _authHeaders() =>
    _authToken != null ? {'authorization': 'Bearer $_authToken'} : {};

Object? arg(CommandArgs args, String key) {
  if (args == null || !args.containsKey(key)) {
    throw ArgumentError("Missing argument '$key'");
  }

  return args[key];
}

Object? argOpt(CommandArgs args, String key) => args?[key];

String _jsString(Object? part) => part == null ? 'null' : part.toString();

String escape(Object? part) => Uri.encodeComponent(_jsString(part));

HttpDescriptor _workflowRunAction(String action) => HttpDescriptor(
      method: (args) => HttpMethod.post,
      path: (args) => 'workflow_runs/${escape(arg(args, 'workflowRunId'))}/$action',
      body: (args) => <String, Object?>{},
    );

HttpDescriptor _workflowRunDebugAction(String action) => HttpDescriptor(
      method: (args) => HttpMethod.post,
      path: (args) => 'workflow_runs/${escape(arg(args, 'workflowRunId'))}/debug/$action',
      body: (args) => <String, Object?>{},
    );

Object? _extractWorkflowRunId(Object? raw) {
  final body = raw is Map ? raw : null;
  final run = body?['run'];
  final id = run is Map ? run['id'] : null;

  if (id is! String || id.isEmpty) {
    throw StateError('missing workflow run id');
  }

  return {'id': id};
}

final Map<String, HttpDescriptor> _registry = {
  'auth_config': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'auth/config'),
  'auth_me': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'auth/me'),
  'login': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'auth/login',
    body: (a) => {'username': arg(a, 'username'), 'password': arg(a, 'password')},
  ),
  'refresh_session': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'auth/refresh',
    body: (a) => {'refresh_token': arg(a, 'refreshToken')},
  ),
  'logout': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'auth/logout',
    body: (a) => {'refresh_token': arg(a, 'refreshToken')},
  ),
  'list_workflow_grants': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'workflows/${escape(arg(a, 'workflowId'))}/grants',
  ),
  'create_workflow_grant': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflows/${escape(arg(a, 'workflowId'))}/grants',
    body: (a) => {
      'principal_type': arg(a, 'principalType'),
      'principal_id': arg(a, 'principalId'),
      'permission': arg(a, 'permission'),
    },
  ),
  'revoke_workflow_grant': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'workflows/${escape(arg(a, 'workflowId'))}/grants/${escape(arg(a, 'grantId'))}',
  ),
  'list_dead_letters': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) {
      final params = <String, String>{};
      final channel = argOpt(a, 'channel');
      if (channel is String && channel.isNotEmpty) {
        params['channel'] = channel;
      }
      final limit = argOpt(a, 'limit');
      if (limit != null) {
        params['limit'] = displayValue(limit);
      }
      final query = Uri(queryParameters: params.isEmpty ? null : params).query;
      return query.isEmpty ? 'dead_letters' : 'dead_letters?$query';
    },
  ),
  'list_audit_log': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) {
      final params = <String, String>{};
      final actorId = argOpt(a, 'actorId');
      if (actorId is String && actorId.isNotEmpty) {
        params['actor_id'] = actorId;
      }
      final action = argOpt(a, 'action');
      if (action is String && action.isNotEmpty) {
        params['action'] = action;
      }
      final limit = argOpt(a, 'limit');
      if (limit != null) {
        params['limit'] = displayValue(limit);
      }
      final query = Uri(queryParameters: params.isEmpty ? null : params).query;
      return query.isEmpty ? 'audit_log' : 'audit_log?$query';
    },
  ),
  'list_users': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'users'),
  'create_user': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'users',
    body: (a) => arg(a, 'request'),
  ),
  'update_user': HttpDescriptor(
    method: (a) => HttpMethod.patch,
    path: (a) => 'users/${escape(arg(a, 'userId'))}',
    body: (a) => arg(a, 'request'),
  ),
  'delete_user': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'users/${escape(arg(a, 'userId'))}',
  ),
  'list_teams': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'teams'),
  'create_team': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'teams',
    body: (a) => {'name': arg(a, 'name')},
  ),
  'update_team': HttpDescriptor(
    method: (a) => HttpMethod.patch,
    path: (a) => 'teams/${escape(arg(a, 'teamId'))}',
    body: (a) => {'name': arg(a, 'name')},
  ),
  'delete_team': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'teams/${escape(arg(a, 'teamId'))}',
  ),
  'list_team_members': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'teams/${escape(arg(a, 'teamId'))}/members',
  ),
  'list_user_teams': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'users/${escape(arg(a, 'userId'))}/teams',
  ),
  'add_team_member': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'teams/${escape(arg(a, 'teamId'))}/members',
    body: (a) => {'user_id': arg(a, 'userId')},
  ),
  'remove_team_member': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'teams/${escape(arg(a, 'teamId'))}/members/${escape(arg(a, 'userId'))}',
  ),
  'list_api_keys': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'api_keys'),
  'create_api_key': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'api_keys',
    body: (a) => arg(a, 'request'),
  ),
  'update_api_key': HttpDescriptor(
    method: (a) => HttpMethod.patch,
    path: (a) => 'api_keys/${escape(arg(a, 'keyId'))}',
    body: (a) => arg(a, 'request'),
  ),
  'revoke_api_key': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'api_keys/${escape(arg(a, 'keyId'))}',
  ),
  'rotate_api_key': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'api_keys/${escape(arg(a, 'keyId'))}/rotate',
  ),
  'fetch_workflows': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'workflows'),
  'save_workflow': HttpDescriptor(
    method: (a) {
      final workflow = arg(a, 'workflow') as Map;
      return workflow['id'] != null ? HttpMethod.patch : HttpMethod.post;
    },
    path: (a) {
      final workflow = arg(a, 'workflow') as Map;
      final id = workflow['id'];
      return id != null ? 'workflows/${escape(id)}' : 'workflows';
    },
    body: (a) => arg(a, 'workflow'),
  ),
  'save_workflow_bundle': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflows/import',
    body: (a) => arg(a, 'request'),
    headers: (a) => {_workflowJsonImportRiskHeader: _workflowJsonImportRiskAck},
  ),
  'save_workflow_wdl': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/import',
    body: (a) => arg(a, 'request'),
  ),
  'delete_workflow': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'workflows/${escape(arg(a, 'workflowId'))}',
  ),
  'duplicate_workflow': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) {
      final bump = argOpt(a, 'bump') ?? 'minor';
      return 'workflows/${escape(arg(a, 'workflowId'))}/duplicate?bump=${escape(bump)}';
    },
  ),
  'fetch_workflow_triggers': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'workflows/${escape(arg(a, 'workflowId'))}/triggers',
  ),
  'save_workflow_trigger': HttpDescriptor(
    method: (a) => arg(a, 'creating') == true ? HttpMethod.post : HttpMethod.patch,
    path: (a) {
      final creating = arg(a, 'creating') == true;
      final trigger = arg(a, 'trigger') as Map;

      if (creating) {
        return 'workflows/${escape(trigger['workflow_id'])}/triggers';
      }

      final id = trigger['id'];
      if (id == null) {
        throw StateError('missing workflow trigger id');
      }

      return 'workflow_triggers/${escape(id)}';
    },
    body: (a) => arg(a, 'trigger'),
  ),
  'delete_workflow_trigger': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'workflow_triggers/${escape(arg(a, 'triggerId'))}',
  ),
  'fetch_run_chunks': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'runs/${escape(arg(a, 'runId'))}/chunks?limit=500',
  ),
  'fetch_run_artifacts': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'runs/${escape(arg(a, 'runId'))}/artifacts',
  ),
  'fetch_workflow_node_run_chunks': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'workflow_node_runs/${escape(arg(a, 'nodeRunId'))}/chunks?limit=500',
  ),
  'fetch_workflow_node_run_artifacts': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'workflow_node_runs/${escape(arg(a, 'nodeRunId'))}/artifacts',
  ),
  'fetch_workflow_run_artifacts': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}/artifacts',
  ),
  'create_workflow_run': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflows/${escape(arg(a, 'workflowId'))}/runs',
    body: (a) => {
      'debug': argOpt(a, 'debug') ?? false,
      'parameters': argOpt(a, 'parameters') ?? <String, Object?>{},
    },
    transform: _extractWorkflowRunId,
  ),
  'step_workflow_run': _workflowRunDebugAction('step'),
  'continue_workflow_run': _workflowRunDebugAction('continue'),
  'cancel_workflow_run': _workflowRunAction('cancel'),
  'pause_workflow_run': _workflowRunAction('pause'),
  'resume_workflow_run': _workflowRunAction('resume'),
  'patch_workflow_run_debug': HttpDescriptor(
    method: (a) => HttpMethod.patch,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}/debug',
    body: (a) => arg(a, 'patch'),
  ),
  'run_to_cursor_workflow_run': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}/debug/run_to_cursor',
    body: (a) => {'node_id': arg(a, 'nodeId')},
  ),
  'skip_workflow_node': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}/debug/skip',
    body: (a) => {
      'output_json': arg(a, 'outputJson'),
      'message': argOpt(a, 'message'),
    },
  ),
  'resolve_workflow_input': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflow_node_runs/${escape(arg(a, 'nodeRunId'))}/input',
    body: (a) => {
      'output_json': arg(a, 'outputJson'),
      'resolved_by': argOpt(a, 'resolvedBy'),
      'message': argOpt(a, 'message'),
    },
  ),
  'rerun_workflow_node': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}/debug/rerun_node',
    body: (a) => {'parameters': arg(a, 'parameters')},
  ),
  'fetch_supervisor_status': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'supervisor/status',
    accept404: true,
  ),
  'replay_workflow_run': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}/replay',
    body: (a) => {'from_step_id': argOpt(a, 'fromStepId')},
    transform: _extractWorkflowRunId,
  ),
  'rename_workflow_run': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}/rename',
    body: (a) => {'name': argOpt(a, 'name')},
  ),
  'fetch_workflow_runs': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) {
      final workflowId = argOpt(a, 'workflowId');
      return workflowId != null
          ? 'workflow_runs?workflow_id=${escape(workflowId)}'
          : 'workflow_runs';
    },
  ),
  'fetch_workflow_run': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'workflow_runs/${escape(arg(a, 'workflowRunId'))}',
    transform: (raw) {
      final body = raw is Map ? raw : <String, Object?>{};

      if (body['run'] == null) {
        throw StateError('missing workflow run');
      }

      return {'run': body['run'], 'nodes': body['nodes'] ?? <Object?>[]};
    },
  ),
  'fetch_resource_records': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => arg(a, 'endpoint') as String,
  ),
  'complete_wdl': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/complete',
    body: (a) => arg(a, 'request'),
  ),
  'hover_wdl': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/hover',
    body: (a) => arg(a, 'request'),
  ),
  'compile_wdl': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/compile',
    body: (a) => {'source': arg(a, 'source'), 'enabled': arg(a, 'enabled')},
  ),
  'analyze_wdl': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/analyze',
    body: (a) => {'source': arg(a, 'source'), 'source_path': argOpt(a, 'sourcePath')},
  ),
  'format_wdl': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/format',
    body: (a) => {'source': arg(a, 'source')},
  ),
  'decompile_to_wdl': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/decompile',
    body: (a) => {'workflow': arg(a, 'workflow')},
  ),
  'evaluate_expression': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'wdl/evaluate',
    body: (a) => {'expression': arg(a, 'expression'), 'context': arg(a, 'context')},
  ),
  'fetch_providers': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'providers'),
  'fetch_replicas': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'replicas'),
  'fetch_node_backends': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'nodes/backends'),
  'fetch_nodes': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'nodes'),
  'scale_nodes': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'nodes/scale',
    body: (a) => arg(a, 'request'),
  ),
  'stop_node': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'nodes/stop',
    body: (a) => arg(a, 'request'),
  ),
  'fetch_credentials': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'credentials'),
  'fetch_credential': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) =>
        'credentials?scope=${escape(arg(a, 'scope'))}&name=${escape(arg(a, 'name'))}&kind=${escape(arg(a, 'kind'))}',
  ),
  'save_credential': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'credentials',
    body: (a) => arg(a, 'request'),
  ),
  'delete_credential': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) =>
        'credentials?scope=${escape(arg(a, 'scope'))}&name=${escape(arg(a, 'name'))}&kind=${escape(arg(a, 'kind'))}',
  ),
  'approve_approval': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'approvals/${escape(arg(a, 'approvalId'))}/approve',
    body: (a) => <String, Object?>{},
  ),
  'reject_approval': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'approvals/${escape(arg(a, 'approvalId'))}/reject',
    body: (a) => <String, Object?>{},
  ),
  'fetch_all_artifacts': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'artifacts'),
  'fetch_notifications': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) {
      final limitArg = argOpt(a, 'limit');
      final limit = limitArg is num ? limitArg : 200;
      final unread = argOpt(a, 'unreadOnly') == true;
      final base = 'notifications?limit=${escape(limit)}';
      return unread ? '$base&unread=true' : base;
    },
  ),
  'mark_notification_read': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'notifications/${escape(arg(a, 'notificationId'))}/mark_read',
    body: (a) => <String, Object?>{},
  ),
  'mark_all_notifications_read': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'notifications/mark_all_read',
    body: (a) => <String, Object?>{},
  ),
  'delete_notification': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'notifications/${escape(arg(a, 'notificationId'))}',
  ),
  'delete_artifact': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'artifacts/${escape(arg(a, 'artifactId'))}',
  ),
  'delete_gate': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'gates/${escape(arg(a, 'gateId'))}',
  ),
  'delete_automation_event': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'automation_events/${escape(arg(a, 'eventId'))}',
  ),
  'fetch_replica_samples': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) {
      final base = 'replicas/${escape(arg(a, 'replicaId'))}/samples';
      final since = argOpt(a, 'sinceSeconds');
      return since != null ? '$base?since_seconds=${escape(since)}' : base;
    },
  ),
  'set_workflow_owner': HttpDescriptor(
    method: (a) => HttpMethod.patch,
    path: (a) => 'workflows/${escape(arg(a, 'workflowId'))}/owner',
    body: (a) => {'org_id': argOpt(a, 'orgId')},
  ),
  // --- organizations (tenants), membership, resource allocation, and billing ---
  'list_my_orgs': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'orgs/me'),
  'list_orgs': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'orgs'),
  'create_org': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'orgs',
    body: (a) => {'name': arg(a, 'name')},
  ),
  'switch_org': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'auth/switch-org',
    body: (a) => {'org_id': arg(a, 'orgId')},
  ),
  'list_org_members': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/members',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
  ),
  'add_org_member': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/members',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
    body: (a) => {'user_id': arg(a, 'userId'), 'role': arg(a, 'role')},
  ),
  'update_org_member': HttpDescriptor(
    method: (a) => HttpMethod.patch,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/members/${escape(arg(a, 'userId'))}',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
    body: (a) => {'role': arg(a, 'role')},
  ),
  'remove_org_member': HttpDescriptor(
    method: (a) => HttpMethod.delete,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/members/${escape(arg(a, 'userId'))}',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
  ),
  'fetch_rate_card': HttpDescriptor(method: (a) => HttpMethod.get, path: (a) => 'rate-card'),
  'fetch_org_nodes': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/nodes',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
  ),
  'scale_org_nodes': HttpDescriptor(
    method: (a) => HttpMethod.post,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/nodes/scale',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
    body: (a) => arg(a, 'request'),
  ),
  'fetch_org_quota': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/quota',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
  ),
  'fetch_org_usage': HttpDescriptor(
    method: (a) => HttpMethod.get,
    path: (a) => 'orgs/${escape(arg(a, 'orgId'))}/usage',
    headers: (a) => {'x-org-id': _jsString(arg(a, 'orgId'))},
  ),
};

/// exposed for tests / advanced callers that want to inspect the registry directly.
Map<String, HttpDescriptor> get httpRegistry => _registry;

// compile-time override, mirrors the ts source's import.meta.env.VITE_RUNINATOR_WS_URL.
// pass --dart-define=RUNINATOR_WS_URL=http://127.0.0.1:8080 to set it.
const String _wsUrlOverride = String.fromEnvironment('RUNINATOR_WS_URL');

String apiBaseUrl() {
  if (_wsUrlOverride.trim().isNotEmpty) {
    return _wsUrlOverride.replaceAll(RegExp(r'/+$'), '');
  }

  return '/api';
}

/// mirrors the ts source's `typeof window !== "undefined"` browser-origin fallback.
/// core/ has no browser dependency, so this is left as an injectable hook: a
/// concrete web platform adapter (future UI pass) can call [setWsOriginProvider];
/// with none configured (as in a `dart test` run) this behaves like the ts
/// source's "no window" branch and returns "".
String Function()? _originProvider;

void setWsOriginProvider(String Function() provider) {
  _originProvider = provider;
}

String wsBaseUrl() {
  if (_wsUrlOverride.trim().isNotEmpty) {
    return _wsUrlOverride.replaceAll(RegExp(r'/+$'), '');
  }

  return _originProvider?.call() ?? '';
}

final Dio _dio = Dio();

// unauthenticated reachability probe against the public /health endpoint; resolves false when the
// backend/proxy cannot be reached or reports unhealthy. used to detect idle outages/recovery.
Future<bool> pingBackendHealth() async {
  try {
    final response = await _dio.get<Object?>(
      '${apiBaseUrl()}/health',
      options: Options(validateStatus: (_) => true),
    );
    return response.statusCode != null && response.statusCode! >= 200 && response.statusCode! < 300;
  } on DioException {
    return false;
  }
}

class HttpRuntimeException implements Exception {
  HttpRuntimeException(this.message);

  final String message;

  @override
  String toString() => message;
}

Future<Object?> invokeViaHttp(String name, [Map<String, Object?>? args]) async {
  if (name == 'get_service_status') {
    final origin = wsBaseUrl();
    return {'service_url': origin.isEmpty ? null : origin};
  }

  if (name == 'start_service_discovery') {
    return null;
  }

  if (name == 'set_access_token') {
    setHttpAuthToken(args?['token'] as String?);
    return null;
  }

  if (name == 'upload_artifact' || name == 'download_artifact') {
    throw HttpRuntimeException(
      '$name is not available in web mode; use uploadArtifactBlob/downloadArtifactBlob instead',
    );
  }

  final descriptor = _registry[name];
  if (descriptor == null) {
    throw HttpRuntimeException('Unknown command in web mode: $name');
  }

  final base = apiBaseUrl();
  final path = descriptor.path(args).replaceFirst(RegExp(r'^/+'), '');
  final url = '$base/$path';
  final method = descriptor.method(args);

  final headers = <String, String>{
    ..._authHeaders(),
    if (descriptor.headers != null) ...descriptor.headers!(args),
  };

  Object? body;
  if (descriptor.body != null) {
    headers['content-type'] = 'application/json';
    body = descriptor.body!(args);
  }

  final response = await _dio.request<Object?>(
    url,
    data: body,
    options: Options(
      method: method.wire,
      headers: headers.isEmpty ? null : headers,
      validateStatus: (_) => true,
    ),
  );

  final statusCode = response.statusCode ?? 0;

  if (statusCode == 404 && descriptor.accept404) {
    return response.data;
  }

  if (statusCode < 200 || statusCode >= 300) {
    throw HttpRuntimeException('${method.wire} $url -> $statusCode: ${response.data}');
  }

  if (statusCode == 204) {
    return null;
  }

  final raw = response.data;

  // workflow imports: after import, re-export the first saved workflow to
  // hydrate the bundle with server-assigned ids — mirrors the Tauri command.
  if (name == 'save_workflow_bundle' || name == 'save_workflow_wdl') {
    final saved = raw is Map ? raw : <String, Object?>{};
    final workflows = saved['workflows'];
    final id = workflows is List && workflows.isNotEmpty && workflows.first is Map
        ? (workflows.first as Map)['id']
        : null;

    if (id == null) {
      return saved;
    }

    final exportResponse = await _dio.get<Object?>(
      '$base/workflows/${escape(id)}/export',
      options: Options(headers: _authHeaders().isEmpty ? null : _authHeaders(), validateStatus: (_) => true),
    );

    final exportStatus = exportResponse.statusCode ?? 0;
    if (exportStatus < 200 || exportStatus >= 300) {
      throw HttpRuntimeException(
        'GET workflows/$id/export -> $exportStatus: ${exportResponse.data}',
      );
    }

    return exportResponse.data;
  }

  return descriptor.transform != null ? descriptor.transform!(raw) : raw;
}
