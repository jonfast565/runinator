// port of core/workflow/index.ts (verbatim, per explicit user direction — this is
// graph-canvas editing/layout logic that only a future Flutter WorkflowCanvas UI
// will exercise, but it is ported in full now rather than deferred).
//
// JsonRecord (Map<String, Object?>) is used throughout exactly as the ts source
// uses its loosely-typed JsonRecord for in-place graph/definition editing;
// mutations write back into the same map references, mirroring the source's
// object-mutation style.

import 'dart:convert';

import '../domain/icons.dart';
import '../domain/json.dart';
import '../domain/models/index.dart';
import '../utils/status.dart';
import '../utils/values.dart';
import 'graph_model.dart';

T? _firstWhereOrNull<T>(Iterable<T> items, bool Function(T) test) {
  for (final item in items) {
    if (test(item)) {
      return item;
    }
  }

  return null;
}

double _jsNumber(Object? value) {
  if (value == null) {
    return 0;
  }

  if (value is num) {
    return value.toDouble();
  }

  if (value is bool) {
    return value ? 1 : 0;
  }

  if (value is String) {
    return double.tryParse(value.trim()) ?? double.nan;
  }

  return double.nan;
}

List<String> nodeRefArray(Object? value) {
  if (value is! List) {
    return [];
  }

  return value.map(nodeRefId).whereType<String>().toList();
}


// ===========================================================================
// node-kind registry
// ===========================================================================

final List<String> workflowNodeKinds = [
  'action',
  'approval',
  'gate',
  'signal',
  'loop',
  'condition',
  'wait',
  'switch',
  'toggle',
  'percentage',
  'parallel',
  'join',
  'try',
  'map',
  'race',
  'output',
  'input',
  'config',
  'subflow',
  'assert',
  'transform',
  'audit',
  'checkpoint',
  'mutex',
  'throttle',
  'await_run',
  'debounce',
  'collect',
  'barrier',
  'circuit_breaker',
  'event_source',
];

/// icon name and a one-line description for every node kind, used by node chrome and the palette.
class WorkflowNodeKindInfo {
  const WorkflowNodeKindInfo({required this.icon, required this.description});

  final IconName icon;
  final String description;
}

final Map<String, WorkflowNodeKindInfo> workflowNodeKindInfo = {
  'start': const WorkflowNodeKindInfo(icon: IconName.play, description: 'Entry point where the workflow run begins.'),
  'action': const WorkflowNodeKindInfo(icon: IconName.bolt, description: 'Runs a task through a provider action.'),
  'wait': const WorkflowNodeKindInfo(icon: IconName.clock, description: 'Pauses the run for a fixed delay or until a time.'),
  'condition': const WorkflowNodeKindInfo(icon: IconName.branch, description: 'Routes down a branch based on a boolean expression.'),
  'switch': const WorkflowNodeKindInfo(icon: IconName.switch_, description: 'Routes to one of several cases by matching a value.'),
  'toggle': const WorkflowNodeKindInfo(
    icon: IconName.toggle,
    description: "A light switch: routes to on or off by a value's truthiness.",
  ),
  'percentage': const WorkflowNodeKindInfo(
    icon: IconName.percentage,
    description: 'Weighted rollout: routes to a bucket by a stable hash of a key.',
  ),
  'approval': const WorkflowNodeKindInfo(icon: IconName.approve, description: 'Halts until a human approves or rejects.'),
  'gate': const WorkflowNodeKindInfo(
    icon: IconName.shield,
    description: 'Blocks until an automated/policy check or manual gate opens.',
  ),
  'signal': const WorkflowNodeKindInfo(
    icon: IconName.bell,
    description: 'Pauses until a named external signal is delivered to the run.',
  ),
  'loop': const WorkflowNodeKindInfo(icon: IconName.loop, description: 'Repeats its target node while a condition holds.'),
  'parallel': const WorkflowNodeKindInfo(icon: IconName.parallel, description: 'Fans out into branches that run concurrently.'),
  'join': const WorkflowNodeKindInfo(icon: IconName.join, description: 'Waits for upstream branches to finish before continuing.'),
  'try': const WorkflowNodeKindInfo(icon: IconName.shield, description: 'Guards a body node and catches failures with a handler.'),
  'map': const WorkflowNodeKindInfo(icon: IconName.grid, description: 'Runs its target once for each item in a collection.'),
  'race': const WorkflowNodeKindInfo(icon: IconName.race, description: 'Runs branches concurrently; the first to finish wins.'),
  'output': const WorkflowNodeKindInfo(icon: IconName.output, description: 'Publishes output without interrupting the flow.'),
  'input': const WorkflowNodeKindInfo(icon: IconName.message, description: 'Waits for a user-supplied value from the UI.'),
  'subflow': const WorkflowNodeKindInfo(icon: IconName.workflow, description: 'Invokes another workflow as a nested step.'),
  'config': const WorkflowNodeKindInfo(icon: IconName.gear, description: 'Sets configuration values for downstream nodes.'),
  'end': const WorkflowNodeKindInfo(icon: IconName.flag, description: 'Terminal node that completes the run successfully.'),
  'fail': const WorkflowNodeKindInfo(icon: IconName.alert, description: 'Terminal node that ends the run as failed.'),
  'assert': const WorkflowNodeKindInfo(
    icon: IconName.check,
    description: 'Evaluates named boolean assertions; fails with a structured violation list.',
  ),
  'transform': const WorkflowNodeKindInfo(
    icon: IconName.gear,
    description: 'Resolves named expression bindings into the run context; no side effects.',
  ),
  'audit': const WorkflowNodeKindInfo(
    icon: IconName.file,
    description: 'Appends a tamper-evident audit record to the workflow log.',
  ),
  'checkpoint': const WorkflowNodeKindInfo(
    icon: IconName.save,
    description: 'Snapshots run state at a named point; enables rollback via the control-plane API.',
  ),
  'mutex': const WorkflowNodeKindInfo(
    icon: IconName.lock,
    description: 'Acquires a named distributed mutex; parks until the lock is available.',
  ),
  'throttle': const WorkflowNodeKindInfo(
    icon: IconName.hourglass,
    description: 'Enforces a cross-run rate limit; parks until a token is available.',
  ),
  'await_run': const WorkflowNodeKindInfo(
    icon: IconName.runs,
    description: 'Waits for one or more independently-started runs to reach a terminal state.',
  ),
  'debounce': const WorkflowNodeKindInfo(
    icon: IconName.clock,
    description: 'Parks with a trailing delay that resets on re-trigger; collapses event bursts.',
  ),
  'collect': const WorkflowNodeKindInfo(
    icon: IconName.list,
    description: 'Accumulates externally-delivered items until a count or time threshold is met.',
  ),
  'barrier': const WorkflowNodeKindInfo(
    icon: IconName.join,
    description: 'Parks until N runs reach this named barrier; the last arrival releases all waiters.',
  ),
  'circuit_breaker': const WorkflowNodeKindInfo(
    icon: IconName.shield,
    description: 'Tracks failure rates across runs; fast-fails or routes to fallback when tripped.',
  ),
  'event_source': const WorkflowNodeKindInfo(
    icon: IconName.bell,
    description: 'Subscribes to a named event stream; drives a body subgraph on each matching event.',
  ),
};

IconName workflowNodeKindIcon(String kind) => workflowNodeKindInfo[kind]!.icon;

// human-friendly label for a node kind: the wire value is snake_case (e.g. `await_run`,
// `circuit_breaker`), which reads poorly in the palette/chrome, so render it title-cased.
String workflowNodeKindLabel(String kind) => titleCase(kind);

String workflowNodeKindDescription(String kind) => workflowNodeKindInfo[kind]!.description;

final List<String> directTransitionKeys = ['next', 'on_success', 'on_failure', 'on_timeout', 'on_reject'];
final List<String> workflowConnectionHandles = ['top', 'right', 'bottom', 'left'];
final List<String> workflowEdgeStyles = ['bezier', 'straight', 'square'];
const String _semanticTargetHandleId = 'target:in';

List<GraphNodeModel> buildGraphNodeModels(
  WorkflowDefinition workflow,
  WorkflowRunDetail? detail, {
  Map<String, String>? subflowNames,
  List<ProviderMetadata> providers = const [],
}) {
  final definition = workflow.definition;
  final nodes = recordArray(definition['nodes']);
  final issuesByNode = _validationIssuesByNode(validateWorkflowIssues(definition, providers));
  final layout = workflowLayoutNodes(definition);
  final fallbackLayout = autoArrangeWorkflowLayout(definition);
  final detailNodes = detail?.nodes ?? const <WorkflowNodeRun>[];
  final runByNode = {for (final run in detailNodes) run.nodeId: run};
  final executionCounts = _workflowRunExecutionCounts(detailNodes);
  final debug = DebugFrame.fromCoercedJson(detail?.run.state?['debug']);
  final breakpointSet = (debug?.breakpoints ?? const <String>[]).toSet();

  return List.generate(nodes.length, (index) {
    final node = nodes[index];
    final id = displayValue(node['id']).isNotEmpty ? displayValue(node['id']) : 'step_${index + 1}';
    final layoutPositionRaw = layout[id] ?? fallbackLayout[id];
    final position = layoutPositionRaw is Map<String, Object?>
        ? GraphPosition(x: _jsNumber(layoutPositionRaw['x']), y: _jsNumber(layoutPositionRaw['y']))
        : GraphPosition(x: (index % 4) * 220, y: (index / 4).floor() * 90);
    final run = runByNode[id];
    final status = run?.status ?? _inferredNodeStatus(node, id, detail);
    final kind = _workflowNodeKindOf(node['kind']);

    return GraphNodeModel(
      id: id,
      type: 'workflow',
      position: layoutPositionRaw is Map<String, Object?>
          ? GraphPosition(
              x: _jsNumber(layoutPositionRaw['x'] ?? 0),
              y: _jsNumber(layoutPositionRaw['y'] ?? 0),
            )
          : position,
      data: GraphNodeData(
        title: _nodeDisplayName(node, id),
        nodeId: id,
        kind: kind,
        summary: _nodeSummary(node, subflowNames),
        semanticHandles: workflowNodeSemanticHandles(node),
        inlineEdit: workflowInlineEditDescriptor(node),
        validationIssues: issuesByNode[id] ?? const [],
        validationCount: (issuesByNode[id] ?? const []).length,
        validationSeverity: _validationSeverity(issuesByNode[id] ?? const []),
        statusLabel: run != null ? '${run.status} a${run.attempt}' : status,
        executionCount: executionCounts[id] ?? 0,
        approvalPrompt: _approvalPrompt(node, run?.state),
        inputPrompt: _inputPrompt(node, run?.state),
        running: status == 'running' || status == 'queued',
        status: status,
        protected_: kind == 'start' || kind == 'end' || kind == 'fail',
        locked: kind == 'start' || kind == 'end' || kind == 'fail' || node['locked'] == true,
        skipped: node['skipped'] == true,
        debugBreakpoint: breakpointSet.contains(id),
      ),
      className: statusClassForNode(status),
    );
  });
}

String workflowRunSearchText(RunSummary run, [String workflowName = '']) => [
      run.id,
      run.workflowId ?? '',
      workflowName,
      run.status,
      run.trigger ?? '',
    ].join(' ').toLowerCase();

Map<String, int> _workflowRunExecutionCounts(List<WorkflowNodeRun> nodes) {
  final counts = <String, int>{};

  for (final node in nodes) {
    final executions = _workflowNodeRunExecutionCount(node);

    if (executions <= 0) {
      continue;
    }

    counts[node.nodeId] = (counts[node.nodeId] ?? 0) + executions;
  }

  return counts;
}

int _workflowNodeRunExecutionCount(WorkflowNodeRun node) {
  if (node.attempt > 0) {
    return node.attempt.floor();
  }

  return node.status == 'queued' ? 0 : 1;
}

List<GraphEdgeModel> buildGraphEdgeModels(WorkflowDefinition workflow) {
  final definition = workflow.definition;
  final nodes = recordArray(definition['nodes']);
  final nodeIds = nodes.map((node) => displayValue(node['id'])).toSet();
  final issuesByEdge = _validationIssuesByEdge(validateWorkflowIssues(definition));
  final edges = <GraphEdgeModel>[];

  WorkflowEditorEdgeData edgeData(String source, String semanticKey, WorkflowEditorEdgeData data) {
    final issues = issuesByEdge[_edgeValidationKey(source, semanticKey)] ?? const <WorkflowValidationIssue>[];
    return WorkflowEditorEdgeData(
      kind: data.kind,
      transitionKey: data.transitionKey,
      branchIndex: data.branchIndex,
      parameterKey: data.parameterKey,
      parameterIndex: data.parameterIndex,
      sourceHandle: data.sourceHandle,
      targetHandle: data.targetHandle,
      edgeStyle: data.edgeStyle,
      labelOffset: data.labelOffset,
      labelAnchor: data.labelAnchor,
      parallelOffset: data.parallelOffset,
      validationCount: issues.length,
      validationSeverity: _validationSeverity(issues),
      validationMessages: issues.map((issue) => issue.message).toList(),
      editable: data.editable,
    );
  }

  for (final node in nodes) {
    final source = displayValue(node['id']);
    final transitions = asRecord(node['transitions']);

    for (final key in directTransitionKeys) {
      final target = nodeRefId(transitions[key]);

      if (target != null && nodeIds.contains(target)) {
        final handles = _edgeHandles(definition, source, key);
        edges.add(_graphEdge(
          source,
          target,
          key,
          edgeData(
            source,
            key,
            WorkflowEditorEdgeData(
              kind: WorkflowEditorEdgeKind.direct,
              transitionKey: WorkflowDirectTransitionKey.fromJson(key),
              sourceHandle: handles.sourceHandle,
              targetHandle: handles.targetHandle,
              edgeStyle: handles.edgeStyle,
              labelOffset: handles.labelOffset,
              labelAnchor: handles.labelAnchor,
              editable: true,
            ),
          ),
        ));
      }
    }

    final branches = asArray(transitions['branches']);
    for (var index = 0; index < branches.length; index++) {
      final branch = asRecord(branches[index]);
      final target = nodeRefId(branch['target']);

      if (target != null && nodeIds.contains(target)) {
        final semanticKey = 'branches.$index';
        final handles = _edgeHandles(definition, source, semanticKey);
        final base = displayValue(branch['label']).isNotEmpty
            ? displayValue(branch['label'])
            : 'branch ${index + 1}';
        final label = branch['priority'] is num ? '#${branch['priority']} $base' : base;
        edges.add(_graphEdge(
          source,
          target,
          label,
          edgeData(
            source,
            semanticKey,
            WorkflowEditorEdgeData(
              kind: WorkflowEditorEdgeKind.branch,
              branchIndex: index,
              sourceHandle: handles.sourceHandle,
              targetHandle: handles.targetHandle,
              edgeStyle: handles.edgeStyle,
              labelOffset: handles.labelOffset,
              labelAnchor: handles.labelAnchor,
              editable: true,
            ),
          ),
        ));
      }
    }

    edges.addAll(_controlFlowEdges(definition, node, nodeIds, issuesByEdge));
  }

  return _separateParallelEdges(edges);
}

// control-flow kinds carry their own parameter-based routes; condition has its own branch options;
// terminals and start have no user-defined predicate routes. everything else is a default-transition
// node that can host predicate edges.
final Set<String> _predicateEdgeExcludedKinds = {
  'condition',
  'switch',
  'parallel',
  'race',
  'join',
  'try',
  'loop',
  'map',
  'start',
  'end',
  'fail',
};

bool _supportsPredicateEdges(String kind) => !_predicateEdgeExcludedKinds.contains(kind);

List<WorkflowEdgeSemanticOption> workflowEdgeSemanticOptions(JsonRecord node) {
  final options = directTransitionKeys
      .map((key) => WorkflowEdgeSemanticOption(
            id: 'direct:$key',
            label: _transitionLabel(key),
            description: 'Set $key transition',
          ))
      .toList();
  final kind = _workflowNodeKindOf(node['kind']);
  final transitions = isRecord(node['transitions']) ? node['transitions'] as JsonRecord : <String, Object?>{};

  if (kind == 'condition') {
    final branches = asArray(transitions['branches']);
    for (var index = 0; index < branches.length; index++) {
      options.add(WorkflowEdgeSemanticOption(
        id: 'branch:$index',
        label: 'Condition branch ${index + 1}',
        description: 'Update an existing condition branch',
      ));
    }
    options.add(const WorkflowEdgeSemanticOption(
      id: 'branch:new',
      label: 'New condition branch',
      description: 'Add a conditional route',
    ));
  } else if (_supportsPredicateEdges(kind)) {
    // predicate edges attach a user-defined when -> target route to any default-transition node,
    // evaluated before status routing in ascending priority order.
    final branches = asArray(transitions['branches']);
    for (var index = 0; index < branches.length; index++) {
      options.add(WorkflowEdgeSemanticOption(
        id: 'branch:$index',
        label: 'Predicate edge ${index + 1}',
        description: 'Update a conditional route',
      ));
    }
    options.add(const WorkflowEdgeSemanticOption(
      id: 'branch:new',
      label: 'New predicate edge',
      description: 'Add a conditional route evaluated by priority',
    ));
  }

  final parameters = isRecord(node['parameters']) ? node['parameters'] as JsonRecord : <String, Object?>{};

  if (kind == 'switch') {
    final cases = recordArray(parameters['cases']);
    for (var index = 0; index < cases.length; index++) {
      options.add(WorkflowEdgeSemanticOption(
        id: 'control:cases:$index',
        label: 'Switch case ${index + 1}',
        description: 'Update an existing switch case',
      ));
    }
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:cases:new',
      label: 'New switch case',
      description: 'Add a switch case route',
    ));
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:default',
      label: 'Switch default',
      description: 'Set the default switch route',
    ));
  }

  if (kind == 'toggle') {
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:on',
      label: 'Toggle on',
      description: 'Node routed to when the value is truthy',
    ));
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:off',
      label: 'Toggle off',
      description: 'Node routed to when the value is falsy',
    ));
  }

  if (kind == 'percentage') {
    final buckets = asArray(parameters['buckets']);
    for (var index = 0; index < buckets.length; index++) {
      options.add(WorkflowEdgeSemanticOption(
        id: 'control:buckets:$index',
        label: 'Bucket ${index + 1}',
        description: 'Update an existing percentage bucket target',
      ));
    }
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:default',
      label: 'Percentage default',
      description: 'Fallback route when no bucket matches',
    ));
  }

  if (kind == 'parallel' || kind == 'race') {
    final branches = asArray(parameters['branches']);
    for (var index = 0; index < branches.length; index++) {
      options.add(WorkflowEdgeSemanticOption(
        id: 'control:branches:$index',
        label: '${titleCase(kind)} branch ${index + 1}',
        description: 'Update an existing branch target',
      ));
    }
    options.add(WorkflowEdgeSemanticOption(
      id: 'control:branches:new',
      label: 'New $kind branch',
      description: 'Add a branch target',
    ));
  }

  if (kind == 'join') {
    final dependencies = asArray(parameters['wait_for']);
    for (var index = 0; index < dependencies.length; index++) {
      options.add(WorkflowEdgeSemanticOption(
        id: 'control:wait_for:$index',
        label: 'Join dependency ${index + 1}',
        description: 'Update an existing join dependency',
      ));
    }
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:wait_for:new',
      label: 'New join dependency',
      description: 'Add a node this join waits for',
    ));
  }

  if (kind == 'try') {
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:body',
      label: 'Try body',
      description: 'Set the guarded body node',
    ));
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:catch',
      label: 'Try catch',
      description: 'Set the error handler node',
    ));
    options.add(const WorkflowEdgeSemanticOption(
      id: 'control:finally',
      label: 'Try finally',
      description: 'Set the cleanup node',
    ));
  }

  if (kind == 'loop' || kind == 'map') {
    options.add(WorkflowEdgeSemanticOption(
      id: 'control:target',
      label: '${titleCase(kind)} target',
      description: 'Set the repeated target node',
    ));
  }

  return options;
}

List<WorkflowSemanticHandle> workflowNodeSemanticHandles(JsonRecord node) {
  final handles = <WorkflowSemanticHandle>[
    const WorkflowSemanticHandle(id: _semanticTargetHandleId, label: 'in', type: WorkflowSemanticHandleType.target),
  ];

  for (final option in workflowEdgeSemanticOptions(node)) {
    handles.add(WorkflowSemanticHandle(
      id: _semanticSourceHandleId(option.id),
      label: option.label,
      type: WorkflowSemanticHandleType.source,
      semanticOptionId: option.id,
    ));
  }

  return handles;
}

// every node exposes a free-text display name; actions/configuration are edited in the modal instead.
WorkflowInlineEditDescriptor workflowInlineEditDescriptor(JsonRecord node) => WorkflowInlineEditDescriptor(
      label: 'Name',
      value: displayValue(node['name']),
      valueKind: WorkflowInlineEditValueKind.text,
    );

class NodeEditResult {
  const NodeEditResult.ok(this.nodeId)
      : ok = true,
        message = null;

  const NodeEditResult.error(this.message)
      : ok = false,
        nodeId = null;

  final bool ok;
  final String? nodeId;
  final String? message;
}

NodeEditResult applyWorkflowInlineNodeEdit(
  JsonRecord definition,
  String nodeId,
  String nextId,
  String inlineValue,
) {
  final nodes = recordArray(definition['nodes']);
  final node = _firstWhereOrNull(nodes, (item) => displayValue(item['id']) == nodeId);

  if (node == null) {
    return const NodeEditResult.error('Node no longer exists');
  }

  final trimmedId = nextId.trim();

  if (trimmedId.isEmpty) {
    return const NodeEditResult.error('Node ID is required');
  }

  if (trimmedId != nodeId && nodes.any((item) => displayValue(item['id']) == trimmedId)) {
    return NodeEditResult.error('Node ID $trimmedId already exists');
  }

  if (trimmedId != nodeId) {
    renameWorkflowNodeReferences(definition, nodeId, trimmedId);
  }

  node['id'] = trimmedId;

  // the inline editor only manages the display name; node activity is edited in the step modal.
  final name = inlineValue.trim();

  if (name.isNotEmpty) {
    node['name'] = name;
  } else {
    node.remove('name');
  }

  return NodeEditResult.ok(trimmedId);
}

void renameWorkflowNodeReferences(JsonRecord definition, String previousId, String nextId) {
  if (previousId.isEmpty || nextId.isEmpty || previousId == nextId) {
    return;
  }

  final nodes = recordArray(definition['nodes']);

  if (definition['start'] == previousId) {
    definition['start'] = nextId;
  }

  for (final node in nodes) {
    _renameNodeRefs(node['transitions'], previousId, nextId);
    _renameNodeRefs(node['parameters'], previousId, nextId);
    _renameNodeRefs(node['wait'], previousId, nextId);
    _renameNodeRefs(node['condition'], previousId, nextId);
  }

  _renameWorkflowEdgeHandleSource(definition, previousId, nextId);
}

List<WorkflowValidationIssue> validateWorkflowIssues(
  JsonRecord definition, [
  List<ProviderMetadata> providers = const [],
]) {
  final nodes = recordArray(definition['nodes']);
  final issues = <WorkflowValidationIssue>[];
  final ids = <String, int>{};

  for (final node in nodes) {
    final nodeId = displayValue(node['id']);

    if (nodeId.isEmpty) {
      issues.add(const WorkflowValidationIssue(
        severity: WorkflowValidationSeverity.error,
        nodeId: '<missing>',
        message: 'Node ID is required',
      ));
      continue;
    }

    ids[nodeId] = (ids[nodeId] ?? 0) + 1;
  }

  for (final entry in ids.entries) {
    if (entry.value > 1) {
      issues.add(WorkflowValidationIssue(
        severity: WorkflowValidationSeverity.error,
        nodeId: entry.key,
        message: 'Duplicate node ID ${entry.key}',
      ));
    }
  }

  final nodeIds = ids.keys.toSet();
  final start = definition['start'] is String ? definition['start'] as String : '';

  if (start.isNotEmpty && !nodeIds.contains(start)) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.error,
      nodeId: start,
      message: 'Workflow start references missing node $start',
    ));
  }

  for (final node in nodes) {
    final nodeId = displayValue(node['id']).isNotEmpty ? displayValue(node['id']) : '<missing>';
    final transitions = isRecord(node['transitions']) ? node['transitions'] as JsonRecord : <String, Object?>{};

    for (final key in directTransitionKeys) {
      _pushNodeRefIssue(issues, nodeIds, nodeId, key, transitions[key], false);
    }

    final branches = recordArray(transitions['branches']);
    for (var index = 0; index < branches.length; index++) {
      _pushNodeRefIssue(issues, nodeIds, nodeId, 'branches.$index', branches[index]['target'], true);
    }

    _pushControlFlowIssues(issues, node, nodeIds, nodeId);
    _pushConnectivityIssues(issues, node, nodeId);
    _pushExpressionIssues(issues, node['parameters'], nodeIds, nodeId, '$nodeId.parameters');
    _pushExpressionIssues(issues, node['wait'], nodeIds, nodeId, '$nodeId.wait');
    _pushExpressionIssues(issues, node['condition'], nodeIds, nodeId, '$nodeId.condition');

    for (var index = 0; index < branches.length; index++) {
      _pushExpressionIssues(
        issues,
        branches[index]['when'],
        nodeIds,
        nodeId,
        '$nodeId.transitions.branches[$index].when',
        edgeKey: 'branches.$index',
      );
    }

    _pushProviderIssues(issues, node, providers, nodeId);
  }

  return issues;
}

void _pushConnectivityIssues(List<WorkflowValidationIssue> issues, JsonRecord node, String nodeId) {
  final kind = _workflowNodeKindOf(node['kind']);

  if (kind == 'end' || kind == 'fail') {
    return;
  }

  if (_hasSuccessTransition(node)) {
    return;
  }

  issues.add(WorkflowValidationIssue(
    severity: WorkflowValidationSeverity.error,
    nodeId: nodeId,
    message: '$nodeId has no outgoing path',
  ));
}

String workflowEdgeOptionId(GraphEdgeLike edge) {
  final data = edge.data;

  if (data?.kind == WorkflowEditorEdgeKind.direct && data?.transitionKey != null) {
    return 'direct:${data!.transitionKey!.toJson()}';
  }

  if (data?.kind == WorkflowEditorEdgeKind.branch && data?.branchIndex != null) {
    return 'branch:${data!.branchIndex}';
  }

  if (data?.kind == WorkflowEditorEdgeKind.control && data?.parameterKey != null) {
    return data!.parameterIndex != null
        ? 'control:${data.parameterKey}:${data.parameterIndex}'
        : 'control:${data.parameterKey}';
  }

  return '';
}

WorkflowEdgeEditorDraft? workflowEdgeEditorDraft(WorkflowDefinition workflow, GraphEdgeLike edge) {
  final definition = workflow.definition;
  final nodes = recordArray(definition['nodes']);
  final node = _firstWhereOrNull(nodes, (item) => displayValue(item['id']) == edge.source);

  if (node == null) {
    return null;
  }

  final optionId = workflowEdgeOptionId(edge);

  if (optionId.isEmpty) {
    return null;
  }

  final base = _defaultWorkflowEdgeEditorDraft(edge, optionId);
  final data = edge.data;

  if (data?.kind == WorkflowEditorEdgeKind.branch && data?.branchIndex != null) {
    final branches = asArray(asRecord(node['transitions'])['branches']);
    final branch = asRecord(branches[data!.branchIndex!]);
    return base.copyWith(
      label: displayValue(branch['label']),
      whenJson: _stringifyJson(branch['when'] ?? _defaultConditionBranchWhen()),
      canEditLabel: true,
      canEditCondition: true,
      canMove: true,
      orderIndex: data.branchIndex,
      orderCount: branches.length,
      priority: branch['priority'] is num ? (branch['priority'] as num).toInt() : null,
      canEditPriority: true,
    );
  }

  if (data?.kind == WorkflowEditorEdgeKind.control &&
      data?.parameterKey == 'cases' &&
      data?.parameterIndex != null) {
    final cases = asArray(asRecord(node['parameters'])['cases']);
    final switchCase = asRecord(cases[data!.parameterIndex!]);
    final match = _switchCaseMatchDraft(switchCase);
    return base.copyWith(
      label: displayValue(switchCase['label']),
      matchKind: match.kind,
      matchJson: _stringifyJson(match.value),
      canEditLabel: true,
      canEditSwitchCase: true,
      canMove: true,
      orderIndex: data.parameterIndex,
      orderCount: cases.length,
    );
  }

  if (data?.kind == WorkflowEditorEdgeKind.control && data?.parameterKey != null && data?.parameterIndex != null) {
    final values = asArray(asRecord(node['parameters'])[data!.parameterKey!]);
    return base.copyWith(
      canMove: ['branches', 'wait_for'].contains(data.parameterKey),
      orderIndex: data.parameterIndex,
      orderCount: values.length,
    );
  }

  return base;
}

class EdgeEditResult {
  const EdgeEditResult.ok(this.semanticKey)
      : ok = true,
        message = null;

  const EdgeEditResult.error(this.message)
      : ok = false,
        semanticKey = null;

  final bool ok;
  final String? semanticKey;
  final String? message;
}

EdgeEditResult applyWorkflowEdgeEditorDraft(
  JsonRecord definition,
  GraphEdgeLike? previousEdge,
  WorkflowEdgeEditorDraft draft,
) {
  final parsed = _parseWorkflowEdgeDraftValues(draft);

  if (!parsed.ok) {
    return EdgeEditResult.error(parsed.message!);
  }

  final nodes = recordArray(definition['nodes']);
  final sourceNode = _firstWhereOrNull(nodes, (node) => displayValue(node['id']) == draft.source);

  if (sourceNode == null) {
    return const EdgeEditResult.error('Edge source node no longer exists');
  }

  if (draft.target.isEmpty) {
    return const EdgeEditResult.error('Edge target is required');
  }

  final previousOptionId = previousEdge != null ? workflowEdgeOptionId(previousEdge) : '';

  if (previousEdge != null && (previousEdge.source != draft.source || previousOptionId != draft.optionId)) {
    final previousSourceNode = _firstWhereOrNull(nodes, (node) => displayValue(node['id']) == previousEdge.source);

    if (previousSourceNode != null) {
      removeWorkflowEdge(previousSourceNode, previousEdge);
    }

    _removeEdgeHandlesForEdge(definition, previousEdge);
  }

  final semanticKey = _writeWorkflowEdgeDraft(sourceNode, draft, parsed);

  if (semanticKey == null) {
    return const EdgeEditResult.error('Choose a valid edge type');
  }

  setWorkflowEdgeHandles(
    definition,
    draft.source,
    semanticKey,
    sourceHandle: draft.sourceHandle,
    targetHandle: draft.targetHandle,
    edgeStyle: draft.edgeStyle,
    labelAnchor: WorkflowEdgeLabelAnchor(position: draft.labelAnchor / 100),
  );
  return EdgeEditResult.ok(semanticKey);
}

class MoveEdgeDraftResult {
  const MoveEdgeDraftResult.ok(this.draft)
      : ok = true,
        message = null;

  const MoveEdgeDraftResult.error(this.message)
      : ok = false,
        draft = null;

  final bool ok;
  final WorkflowEdgeEditorDraft? draft;
  final String? message;
}

MoveEdgeDraftResult moveWorkflowEdgeEditorDraft(JsonRecord definition, WorkflowEdgeEditorDraft draft, int direction) {
  final location = _orderedEdgeLocation(definition, draft);

  if (location == null) {
    return const MoveEdgeDraftResult.error('This edge cannot be reordered');
  }

  final nextIndex = location.index + direction;

  if (nextIndex < 0 || nextIndex >= location.items.length) {
    return const MoveEdgeDraftResult.error('Edge is already at that boundary');
  }

  final tmp = location.items[location.index];
  location.items[location.index] = location.items[nextIndex];
  location.items[nextIndex] = tmp;

  _swapWorkflowEdgeHandles(
    definition,
    draft.source,
    location.semanticKey(location.index),
    location.semanticKey(nextIndex),
  );

  return MoveEdgeDraftResult.ok(draft.copyWith(
    optionId: location.optionId(nextIndex),
    edgeId: '',
    orderIndex: nextIndex,
    orderCount: location.items.length,
  ));
}

WorkflowEdgeEditorDraft _defaultWorkflowEdgeEditorDraft(GraphEdgeLike edge, String optionId) {
  final data = edge.data;
  return WorkflowEdgeEditorDraft(
    edgeId: edge.id ?? '',
    source: edge.source,
    target: edge.target,
    optionId: optionId,
    sourceHandle: edge.sourceHandle,
    targetHandle: edge.targetHandle,
    edgeStyle: _normalizeWorkflowEdgeStyle(data?.edgeStyle),
    labelAnchor: ((_normalizeLabelAnchor(data?.labelAnchor)?.position ?? 0.5) * 100).round().toDouble(),
    label: '',
    whenJson: _stringifyJson(_defaultConditionBranchWhen()),
    matchKind: WorkflowEdgeEditorMatchKind.equals,
    matchJson: _stringifyJson(true),
    canEditLabel: false,
    canEditCondition: false,
    canEditSwitchCase: false,
    canMove: false,
    orderIndex: -1,
    orderCount: 0,
    priority: null,
    canEditPriority: false,
  );
}

JsonRecord _defaultConditionBranchWhen() => {
      'value': valueRef('params', ['value']),
      'equals': true,
    };

class _SwitchCaseMatch {
  const _SwitchCaseMatch(this.kind, this.value);

  final WorkflowEdgeEditorMatchKind kind;
  final Object? value;
}

_SwitchCaseMatch _switchCaseMatchDraft(JsonRecord switchCase) {
  if (switchCase.containsKey('when')) {
    return _SwitchCaseMatch(WorkflowEdgeEditorMatchKind.when, switchCase['when'] ?? <String, Object?>{});
  }

  if (switchCase.containsKey('condition')) {
    return _SwitchCaseMatch(WorkflowEdgeEditorMatchKind.when, switchCase['condition'] ?? <String, Object?>{});
  }

  if (switchCase.containsKey('not_equals')) {
    return _SwitchCaseMatch(WorkflowEdgeEditorMatchKind.notEquals, switchCase['not_equals']);
  }

  if (switchCase.containsKey('exists')) {
    return _SwitchCaseMatch(WorkflowEdgeEditorMatchKind.exists, switchCase['exists']);
  }

  return _SwitchCaseMatch(
    WorkflowEdgeEditorMatchKind.equals,
    switchCase.containsKey('equals') ? switchCase['equals'] : true,
  );
}

String _stringifyJson(Object? value) => const JsonEncoder.withIndent('  ').convert(value);

class _ParsedEdgeDraftValues {
  const _ParsedEdgeDraftValues.ok({this.when, this.matchValue})
      : ok = true,
        message = null;

  const _ParsedEdgeDraftValues.error(this.message)
      : ok = false,
        when = null,
        matchValue = null;

  final bool ok;
  final JsonRecord? when;
  final Object? matchValue;
  final String? message;
}

_ParsedEdgeDraftValues _parseWorkflowEdgeDraftValues(WorkflowEdgeEditorDraft draft) {
  if (_isConditionBranchOption(draft.optionId)) {
    final when = _parseDraftJson(draft.whenJson);

    if (!when.ok) {
      return const _ParsedEdgeDraftValues.error('Condition branch predicate must be valid JSON');
    }

    if (!isRecord(when.value)) {
      return const _ParsedEdgeDraftValues.error('Condition branch predicate must be a JSON object');
    }

    return _ParsedEdgeDraftValues.ok(when: when.value as JsonRecord);
  }

  if (_isSwitchCaseOption(draft.optionId)) {
    final match = _parseDraftJson(draft.matchJson);

    if (!match.ok) {
      return const _ParsedEdgeDraftValues.error('Switch case match must be valid JSON');
    }

    return _ParsedEdgeDraftValues.ok(matchValue: match.value);
  }

  return const _ParsedEdgeDraftValues.ok();
}

bool _isConditionBranchOption(String optionId) => optionId.startsWith('branch:');

bool _isSwitchCaseOption(String optionId) => optionId.startsWith('control:cases:');

class _ParsedJson {
  const _ParsedJson.ok(this.value) : ok = true;

  const _ParsedJson.error()
      : ok = false,
        value = null;

  final bool ok;
  final Object? value;
}

_ParsedJson _parseDraftJson(String text) {
  try {
    return _ParsedJson.ok(jsonDecode(text));
  } catch (_) {
    return const _ParsedJson.error();
  }
}

String? _writeWorkflowEdgeDraft(JsonRecord node, WorkflowEdgeEditorDraft draft, _ParsedEdgeDraftValues parsed) {
  if (draft.optionId.startsWith('direct:')) {
    final key = draft.optionId.substring('direct:'.length);

    if (!directTransitionKeys.contains(key)) {
      return null;
    }

    final transitions = asRecord(node['transitions']);
    node['transitions'] = transitions;
    transitions[key] = nodeRef(draft.target);
    return key;
  }

  if (draft.optionId.startsWith('branch:')) {
    final transitions = asRecord(node['transitions']);
    node['transitions'] = transitions;
    final branches = asArray(transitions['branches']);
    transitions['branches'] = branches;
    final index = _edgeOptionIndex(draft.optionId, 'branch', branches.length);

    if (index == null) {
      return null;
    }

    final previous = asRecord(index < branches.length ? branches[index] : null);
    final branch = <String, Object?>{
      ...previous,
      'when': parsed.when ?? (isRecord(previous['when']) ? previous['when'] : _defaultConditionBranchWhen()),
      'target': nodeRef(draft.target),
    };
    _applyOptionalLabel(branch, draft.label);
    _applyBranchPriority(branch, draft, branches, index);

    if (index < branches.length) {
      branches[index] = branch;
    } else {
      branches.add(branch);
    }

    return 'branches.$index';
  }

  if (!draft.optionId.startsWith('control:')) {
    return null;
  }

  final parameters = asRecord(node['parameters']);
  node['parameters'] = parameters;
  final controlParts = draft.optionId.split(':');

  if (controlParts.length < 2 || controlParts[1].isEmpty) {
    return null;
  }

  final parameterKey = controlParts[1];

  if (controlParts.length > 2) {
    final rawIndex = controlParts[2];
    final list = asArray(parameters[parameterKey]);
    parameters[parameterKey] = list;
    final index = rawIndex == 'new' ? list.length : int.tryParse(rawIndex);

    if (index == null || index < 0) {
      return null;
    }

    if (parameterKey == 'cases') {
      final previous = asRecord(index < list.length ? list[index] : null);
      final keysToRemove = {'equals', 'not_equals', 'exists', 'when', 'condition'};
      final switchCase = <String, Object?>{
        for (final entry in {...previous, 'target': nodeRef(draft.target)}.entries)
          if (!keysToRemove.contains(entry.key)) entry.key: entry.value,
      };

      switchCase[draft.matchKind.wire] = asJsonValue(parsed.matchValue ?? true);
      _applyOptionalLabel(switchCase, draft.label);

      if (index < list.length) {
        list[index] = switchCase;
      } else {
        list.add(switchCase);
      }
    } else if (parameterKey == 'buckets') {
      // preserve the bucket's weight; only its target is edited from the canvas.
      final previous = asRecord(index < list.length ? list[index] : null);
      final updated = {...previous, 'target': nodeRef(draft.target)};

      if (index < list.length) {
        list[index] = updated;
      } else {
        list.add(updated);
      }
    } else {
      if (index < list.length) {
        list[index] = nodeRef(draft.target);
      } else {
        list.add(nodeRef(draft.target));
      }
    }

    return parameterSemanticKey(parameterKey, index);
  }

  parameters[parameterKey] = nodeRef(draft.target);
  return parameterSemanticKey(parameterKey);
}

void _applyOptionalLabel(JsonRecord record, String label) {
  final trimmed = label.trim();

  if (trimmed.isNotEmpty) {
    record['label'] = trimmed;
  } else {
    record.remove('label');
  }
}

// write a predicate edge's selection priority (lower is evaluated first). an unset draft priority
// on a new branch defaults to the next free slot after the highest existing priority.
void _applyBranchPriority(JsonRecord branch, WorkflowEdgeEditorDraft draft, List<Object?> branches, int index) {
  if (draft.priority != null) {
    branch['priority'] = draft.priority;
    return;
  }

  final isNew = index >= branches.length || !isRecord(branches[index]);

  if (!isNew) {
    branch.remove('priority');
    return;
  }

  var highest = 0;
  for (final item in branches) {
    final value = isRecord(item) && (item as JsonRecord)['priority'] is num
        ? (item['priority'] as num).toInt()
        : null;
    if (value != null && value > highest) {
      highest = value;
    }
  }

  branch['priority'] = highest + 1;
}

int? _edgeOptionIndex(String optionId, String prefix, int newIndex) {
  final raw = optionId.substring(prefix.length + 1);

  if (raw == 'new') {
    return newIndex;
  }

  final index = int.tryParse(raw);
  return (index != null && index >= 0) ? index : null;
}

class _OrderedEdgeLocation {
  const _OrderedEdgeLocation({
    required this.items,
    required this.index,
    required this.semanticKey,
    required this.optionId,
  });

  final List<Object?> items;
  final int index;
  final String Function(int index) semanticKey;
  final String Function(int index) optionId;
}

_OrderedEdgeLocation? _orderedEdgeLocation(JsonRecord definition, WorkflowEdgeEditorDraft draft) {
  final nodes = recordArray(definition['nodes']);
  final node = _firstWhereOrNull(nodes, (item) => displayValue(item['id']) == draft.source);

  if (node == null) {
    return null;
  }

  if (draft.optionId.startsWith('branch:')) {
    final rawBranches = asRecord(node['transitions'])['branches'];
    final branches = rawBranches is List ? rawBranches : null;
    final index = _edgeOptionIndex(draft.optionId, 'branch', -1);

    if (branches == null || index == null) {
      return null;
    }

    return _OrderedEdgeLocation(
      items: branches,
      index: index,
      semanticKey: (nextIndex) => 'branches.$nextIndex',
      optionId: (nextIndex) => 'branch:$nextIndex',
    );
  }

  if (!draft.optionId.startsWith('control:')) {
    return null;
  }

  final parts = draft.optionId.split(':');
  final parameterKey = parts.length > 1 ? parts[1] : '';
  final rawIndex = parts.length > 2 ? parts[2] : '';

  if (!['cases', 'branches', 'wait_for'].contains(parameterKey) || rawIndex == 'new') {
    return null;
  }

  final rawItems = asRecord(node['parameters'])[parameterKey];
  final items = rawItems is List ? rawItems : null;
  final index = int.tryParse(rawIndex);

  if (items == null || index == null || index < 0) {
    return null;
  }

  return _OrderedEdgeLocation(
    items: items,
    index: index,
    semanticKey: (nextIndex) => parameterSemanticKey(parameterKey, nextIndex),
    optionId: (nextIndex) => 'control:$parameterKey:$nextIndex',
  );
}

String? applyWorkflowEdgeSemantic(JsonRecord node, String target, String optionId) {
  if (target.isEmpty) {
    return null;
  }

  if (optionId.startsWith('direct:')) {
    final key = optionId.substring('direct:'.length);

    if (!directTransitionKeys.contains(key)) {
      return null;
    }

    final transitions = asRecord(node['transitions']);
    node['transitions'] = transitions;
    transitions[key] = nodeRef(target);
    return key;
  }

  if (optionId.startsWith('branch:')) {
    final transitions = asRecord(node['transitions']);
    node['transitions'] = transitions;
    final branches = asArray(transitions['branches']);
    transitions['branches'] = branches;
    final rawIndex = optionId.substring('branch:'.length);
    final index = rawIndex == 'new' ? branches.length : int.tryParse(rawIndex);

    if (index == null || index < 0) {
      return null;
    }

    final previous = asRecord(index < branches.length ? branches[index] : null);
    final next = <String, Object?>{
      'when': isRecord(previous['when'])
          ? previous['when']
          : {
              'value': valueRef('params', ['value']),
              'equals': true,
            },
      'target': nodeRef(target),
    };

    if (index < branches.length) {
      branches[index] = next;
    } else {
      branches.add(next);
    }

    return 'branches.$index';
  }

  if (!optionId.startsWith('control:')) {
    return null;
  }

  final parameters = asRecord(node['parameters']);
  node['parameters'] = parameters;
  final controlParts = optionId.split(':');

  if (controlParts.length < 2 || controlParts[1].isEmpty) {
    return null;
  }

  final parameterKey = controlParts[1];

  if (controlParts.length > 2) {
    final rawIndex = controlParts[2];
    final list = asArray(parameters[parameterKey]);
    parameters[parameterKey] = list;
    final index = rawIndex == 'new' ? list.length : int.tryParse(rawIndex);

    if (index == null || index < 0) {
      return null;
    }

    if (parameterKey == 'cases') {
      final previous = asRecord(index < list.length ? list[index] : null);
      final switchCase = <String, Object?>{...previous, 'target': nodeRef(target)};

      if (index < list.length) {
        list[index] = switchCase;
      } else {
        list.add(switchCase);
      }

      if (!switchCase.containsKey('equals') &&
          !switchCase.containsKey('not_equals') &&
          !switchCase.containsKey('exists') &&
          !switchCase.containsKey('when')) {
        switchCase['equals'] = true;
      }
    } else {
      if (index < list.length) {
        list[index] = nodeRef(target);
      } else {
        list.add(nodeRef(target));
      }
    }

    return parameterSemanticKey(parameterKey, index);
  }

  parameters[parameterKey] = nodeRef(target);
  return parameterSemanticKey(parameterKey);
}

Map<String, WorkflowLayoutPosition> autoArrangeWorkflowLayout(
  JsonRecord definition, [
  WorkflowLayoutDirection direction = WorkflowLayoutDirection.horizontal,
]) {
  final nodes = recordArray(definition['nodes']);
  final ids = <String>[];
  for (var index = 0; index < nodes.length; index++) {
    final id = displayValue(nodes[index]['id']).isNotEmpty ? displayValue(nodes[index]['id']) : 'step_${index + 1}';
    if (id.isNotEmpty) {
      ids.add(id);
    }
  }

  if (ids.isEmpty) {
    return {};
  }

  final nodeIds = ids.toSet();
  final indexById = {for (var i = 0; i < ids.length; i++) ids[i]: i};
  final edges = _workflowLayoutEdges(nodes, nodeIds);
  final components = _stronglyConnectedComponents(ids, edges);
  final componentById = <String, int>{};
  for (var componentIndex = 0; componentIndex < components.length; componentIndex++) {
    for (final id in components[componentIndex]) {
      componentById[id] = componentIndex;
    }
  }

  final componentEdges = <int, Set<int>>{};
  final incomingCounts = <int, int>{};
  for (var index = 0; index < components.length; index++) {
    componentEdges[index] = {};
    incomingCounts[index] = 0;
  }

  for (final edge in edges) {
    final sourceComponent = componentById[edge.source];
    final targetComponent = componentById[edge.target];

    if (sourceComponent == null || targetComponent == null || sourceComponent == targetComponent) {
      continue;
    }

    final targets = componentEdges[sourceComponent];

    if (targets == null || targets.contains(targetComponent)) {
      continue;
    }

    targets.add(targetComponent);
    incomingCounts[targetComponent] = (incomingCounts[targetComponent] ?? 0) + 1;
  }

  final levels = _componentLevels(components, componentEdges, incomingCounts, definition['start'], indexById);
  final maxLevel = levels.isEmpty ? 0 : levels.reduce((a, b) => a > b ? a : b).clamp(0, 1 << 30);
  final grouped = List.generate(maxLevel + 1, (_) => <int>[]);
  for (var componentIndex = 0; componentIndex < levels.length; componentIndex++) {
    grouped[levels[componentIndex]].add(componentIndex);
  }

  for (final group in grouped) {
    group.sort((left, right) =>
        _componentSortKey(components[left], indexById).compareTo(_componentSortKey(components[right], indexById)));
  }

  const columnGap = 270;
  const rowGap = 150;
  final levelSlots = grouped
      .map((group) => group.fold<int>(0, (total, componentIndex) => total + components[componentIndex].length))
      .toList();
  final maxSlots = levelSlots.isEmpty ? 1 : levelSlots.reduce((a, b) => a > b ? a : b).clamp(1, 1 << 30);
  final positions = <String, WorkflowLayoutPosition>{};

  for (var level = 0; level < grouped.length; level++) {
    final group = grouped[level];
    var slot = 0;
    final yOffset = ((maxSlots - levelSlots[level]) * rowGap) / 2;

    for (final componentIndex in group) {
      final component = [...components[componentIndex]]
        ..sort((left, right) => (indexById[left] ?? 0).compareTo(indexById[right] ?? 0));

      for (final id in component) {
        final primary = level * columnGap;
        final secondary = yOffset + slot * rowGap;
        positions[id] = direction == WorkflowLayoutDirection.vertical
            ? WorkflowLayoutPosition(x: secondary, y: primary.toDouble())
            : WorkflowLayoutPosition(x: primary.toDouble(), y: secondary);
        slot += 1;
      }
    }
  }

  return positions;
}

void autoArrangeWorkflowEdgeHandles(JsonRecord definition, Map<String, WorkflowLayoutPosition> positions) {
  final nodes = recordArray(definition['nodes']);
  final nodeIds = nodes.map((node) => displayValue(node['id'])).where((id) => id.isNotEmpty).toSet();

  void setHandles(String source, String semanticKey, String? target) {
    if (target == null || !nodeIds.contains(source) || !nodeIds.contains(target)) {
      return;
    }

    final handles = _connectionHandlesForPositions(positions[source], positions[target]);
    final style = _edgeHandles(definition, source, semanticKey).edgeStyle;
    setWorkflowEdgeHandles(
      definition,
      source,
      semanticKey,
      sourceHandle: handles.sourceHandle,
      targetHandle: handles.targetHandle,
      edgeStyle: style,
    );
  }

  for (final node in nodes) {
    final source = displayValue(node['id']);
    final transitions = isRecord(node['transitions']) ? node['transitions'] as JsonRecord : <String, Object?>{};

    for (final key in directTransitionKeys) {
      setHandles(source, key, nodeRefId(transitions[key]));
    }

    final branches = recordArray(transitions['branches']);
    for (var index = 0; index < branches.length; index++) {
      setHandles(source, 'branches.$index', nodeRefId(branches[index]['target']));
    }

    for (final targetValue in _controlFlowTargetValues(node)) {
      setHandles(source, parameterSemanticKey(targetValue.parameterKey, targetValue.parameterIndex), targetValue.target);
    }
  }
}

WorkflowEditorNodeRecord createWorkflowNode(String kind, List<JsonRecord> nodes) {
  final id = uniqueWorkflowNodeId(nodes, kind);
  final node = <String, Object?>{
    'id': id,
    'kind': kind,
    'parameters': <String, Object?>{},
    'retry': {'max_attempts': 1},
    'transitions': <String, Object?>{},
  };

  switch (kind) {
    case 'action':
      node['action'] = {'provider': '', 'function': '', 'timeout_seconds': 300, 'configuration': <String, Object?>{}};
      break;
    case 'approval':
      node['parameters'] = {'approval_type': 'generic', 'prompt': 'Approval required'};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_reject': nodeRef('end')};
      break;
    case 'gate':
      node['parameters'] = {'kind': 'manual', 'poll_interval': 30};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'signal':
      node['parameters'] = {'name': 'signal'};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'loop':
      node['parameters'] = {'items': <Object?>[], 'target': nodeRef('end')};
      node['max_iterations'] = 10;
      break;
    case 'condition':
      node['condition'] = <String, Object?>{};
      node['transitions'] = {
        'branches': [
          {
            'when': {
              'value': valueRef('params', ['approved']),
              'equals': true,
            },
            'target': nodeRef('end'),
          },
        ],
        'next': nodeRef('end'),
      };
      break;
    case 'wait':
      node['wait'] = {'seconds': 60};
      break;
    case 'switch':
      node['parameters'] = {
        'value': valueRef('params', ['mode']),
        'cases': <Object?>[],
        'default': nodeRef('end'),
      };
      break;
    case 'toggle':
      node['parameters'] = {
        'value': valueRef('config', ['flags', 'enabled']),
        'on': nodeRef('end'),
        'off': nodeRef('end'),
      };
      break;
    case 'percentage':
      node['parameters'] = {
        'key': valueRef('input', ['user_id']),
        'buckets': <Object?>[],
        'default': nodeRef('end'),
      };
      break;
    case 'parallel':
    case 'race':
      node['parameters'] = {'branches': <Object?>[]};
      break;
    case 'join':
      node['parameters'] = {'wait_for': <Object?>[], 'mode': 'all'};
      break;
    case 'try':
      node['parameters'] = <String, Object?>{};
      break;
    case 'map':
      node['parameters'] = {'items': <Object?>[], 'target': nodeRef('end'), 'concurrency': 1};
      break;
    case 'output':
      node['parameters'] = {'event_type': 'workflow.output', 'data': <String, Object?>{}};
      break;
    case 'input':
      node['parameters'] = {'prompt': 'Provide input'};
      break;
    case 'config':
      node['parameters'] = {'name': '', 'metadata': <String, Object?>{}};
      break;
    case 'subflow':
      node['subflow_id'] = '';
      break;
    case 'assert':
      node['parameters'] = {'assertions': <Object?>[]};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'transform':
      node['parameters'] = {'bindings': <String, Object?>{}};
      node['transitions'] = {'next': nodeRef('end')};
      break;
    case 'audit':
      node['parameters'] = {'action': ''};
      node['transitions'] = {'next': nodeRef('end')};
      break;
    case 'checkpoint':
      node['parameters'] = {'name': 'checkpoint'};
      node['transitions'] = {'next': nodeRef('end')};
      break;
    case 'mutex':
      node['parameters'] = {'name': 'my-mutex'};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'throttle':
      node['parameters'] = {'name': 'my-throttle', 'max_per_window': 10, 'window_seconds': 60};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'await_run':
      node['parameters'] = {'run_ids': <Object?>[], 'mode': 'all'};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'debounce':
      node['parameters'] = {'name': 'my-debounce', 'delay_seconds': 30};
      node['transitions'] = {'on_success': nodeRef('end')};
      break;
    case 'collect':
      node['parameters'] = {'name': 'my-collect', 'max': 10};
      node['transitions'] = {'on_success': nodeRef('end')};
      break;
    case 'barrier':
      node['parameters'] = {'name': 'my-barrier', 'count': 2};
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'circuit_breaker':
      node['parameters'] = {
        'name': 'my-circuit-breaker',
        'threshold': 5,
        'window_seconds': 60,
        'cooldown_seconds': 30,
      };
      node['transitions'] = {'on_success': nodeRef('end'), 'on_failure': nodeRef('end')};
      break;
    case 'event_source':
      node['parameters'] = {'event_type': ''};
      node['transitions'] = {'on_success': nodeRef('end')};
      break;
  }

  return node;
}

String uniqueWorkflowNodeId(List<JsonRecord> nodes, String base) => _uniqueNodeId(
      base.replaceAll(RegExp(r'[^a-zA-Z0-9_]+'), '_').isNotEmpty
          ? base.replaceAll(RegExp(r'[^a-zA-Z0-9_]+'), '_')
          : 'node',
      nodes.map((node) => displayValue(node['id'])).where((id) => id.isNotEmpty).toSet(),
    );

String addDirectTransition(JsonRecord node, String target, [String? preferredKey]) {
  final key = (preferredKey != null && directTransitionKeys.contains(preferredKey))
      ? preferredKey
      : _firstAvailableTransition(node);
  final transitions = asRecord(node['transitions']);
  node['transitions'] = transitions;
  transitions[key] = nodeRef(target);
  return key;
}

bool isSameConnectionPointLoop({String? source, String? target, String? sourceHandle, String? targetHandle}) =>
    source != null &&
    target != null &&
    source == target &&
    sourceHandle != null &&
    targetHandle != null &&
    sourceHandle == targetHandle;

void setWorkflowEdgeHandles(
  JsonRecord definition,
  String source,
  String semanticKey, {
  String? sourceHandle,
  String? targetHandle,
  WorkflowEdgeStyle? edgeStyle,
  Object? labelOffset = _unsetHandle,
  Object? labelAnchor = _unsetHandle,
}) {
  final ui = asRecord(definition['ui']);
  definition['ui'] = ui;
  final edgeHandles = asRecord(ui['edge_handles']);
  ui['edge_handles'] = edgeHandles;
  final key = _edgeHandleKey(source, semanticKey);
  // missing label metadata preserves manual placement; null/default clears it.
  final previousOffset = _normalizeLabelOffset(asRecord(edgeHandles[key])['labelOffset']);
  final nextOffset = identical(labelOffset, _unsetHandle)
      ? previousOffset
      : _normalizeLabelOffset(labelOffset as WorkflowEdgeLabelOffset?);
  final previousAnchor = _normalizeLabelAnchor(asRecord(edgeHandles[key])['labelAnchor']);
  final nextAnchor = identical(labelAnchor, _unsetHandle)
      ? previousAnchor
      : _normalizeLabelAnchor(labelAnchor as WorkflowEdgeLabelAnchor?);
  edgeHandles[key] = asJsonValue({
    'sourceHandle': _normalizeConnectionHandle(sourceHandle),
    'targetHandle': _normalizeConnectionHandle(targetHandle),
    'edgeStyle': _normalizeWorkflowEdgeStyle(edgeStyle).toJson(),
    if (nextOffset != null) 'labelOffset': nextOffset.toJson(),
    if (nextAnchor != null) 'labelAnchor': nextAnchor.toJson(),
  });
}

const Object _unsetHandle = Object();

void removeWorkflowEdgeHandles(JsonRecord definition, String source, String semanticKey) {
  final handles = asRecord(definition['ui'])['edge_handles'];

  if (!isRecord(handles)) {
    return;
  }

  final key = _edgeHandleKey(source, semanticKey);
  asRecord(definition['ui'])['edge_handles'] = {
    for (final entry in (handles as JsonRecord).entries)
      if (entry.key != key) entry.key: entry.value,
  };
}

String workflowEdgeSemanticKey(GraphEdgeLike edge) {
  final data = edge.data;

  if (data?.transitionKey != null) {
    return data!.transitionKey!.toJson();
  }

  if (data?.branchIndex != null) {
    return 'branches.${data!.branchIndex}';
  }

  return parameterSemanticKey(data?.parameterKey, data?.parameterIndex);
}

void setWorkflowEdgeLabelOffset(JsonRecord definition, GraphEdgeLike edge, WorkflowEdgeLabelOffset? labelOffset) {
  final data = edge.data;
  final semanticKey = workflowEdgeSemanticKey(edge);

  if (semanticKey.isEmpty) {
    return;
  }

  setWorkflowEdgeHandles(
    definition,
    edge.source,
    semanticKey,
    sourceHandle: edge.sourceHandle,
    targetHandle: edge.targetHandle,
    edgeStyle: _normalizeWorkflowEdgeStyle(data?.edgeStyle),
    labelOffset: labelOffset,
  );
}

void setWorkflowEdgeLabelAnchor(JsonRecord definition, GraphEdgeLike edge, WorkflowEdgeLabelAnchor? labelAnchor) {
  final data = edge.data;
  final semanticKey = workflowEdgeSemanticKey(edge);

  if (semanticKey.isEmpty) {
    return;
  }

  setWorkflowEdgeHandles(
    definition,
    edge.source,
    semanticKey,
    sourceHandle: edge.sourceHandle,
    targetHandle: edge.targetHandle,
    edgeStyle: _normalizeWorkflowEdgeStyle(data?.edgeStyle),
    labelAnchor: labelAnchor,
  );
}

void _removeEdgeHandlesForEdge(JsonRecord definition, GraphEdgeLike edge) {
  final data = edge.data;

  if (data?.transitionKey != null) {
    removeWorkflowEdgeHandles(definition, edge.source, data!.transitionKey!.toJson());
  }

  if (data?.branchIndex != null) {
    removeWorkflowEdgeHandles(definition, edge.source, 'branches.${data!.branchIndex}');
  }

  if (data?.parameterKey != null) {
    removeWorkflowEdgeHandles(definition, edge.source, parameterSemanticKey(data!.parameterKey, data.parameterIndex));
  }
}

void _swapWorkflowEdgeHandles(JsonRecord definition, String source, String leftSemanticKey, String rightSemanticKey) {
  final handles = asRecord(definition['ui'])['edge_handles'];

  if (!isRecord(handles)) {
    return;
  }

  final leftKey = _edgeHandleKey(source, leftSemanticKey);
  final rightKey = _edgeHandleKey(source, rightSemanticKey);
  final record = handles as JsonRecord;
  final left = record[leftKey];
  final right = record[rightKey];
  var next = {...record};

  if (!record.containsKey(rightKey)) {
    next.remove(leftKey);
  } else {
    next[leftKey] = right;
  }

  if (!record.containsKey(leftKey)) {
    next.remove(rightKey);
  } else {
    next[rightKey] = left;
  }

  asRecord(definition['ui'])['edge_handles'] = next;
}

void _renameWorkflowEdgeHandleSource(JsonRecord definition, String previousId, String nextId) {
  final handles = asRecord(definition['ui'])['edge_handles'];

  if (!isRecord(handles)) {
    return;
  }

  final record = handles as JsonRecord;
  final prefix = '$previousId:';
  var next = {...record};

  for (final key in record.keys.toList()) {
    if (!key.startsWith(prefix)) {
      continue;
    }

    final nextKey = '$nextId:${key.substring(prefix.length)}';

    if (!next.containsKey(nextKey)) {
      next[nextKey] = next[key];
    }

    next.remove(key);
  }

  asRecord(definition['ui'])['edge_handles'] = next;
}

bool removeEditableEdge(JsonRecord node, GraphEdgeLike edge) {
  final data = edge.data;

  if (data?.editable != true || !isRecord(node['transitions'])) {
    return false;
  }

  final transitions = node['transitions'] as JsonRecord;

  if (data!.kind == WorkflowEditorEdgeKind.direct &&
      data.transitionKey != null &&
      nodeRefId(transitions[data.transitionKey!.toJson()]) == edge.target) {
    node['transitions'] = {
      for (final entry in transitions.entries)
        if (entry.key != data.transitionKey!.toJson()) entry.key: entry.value,
    };
    return true;
  }

  if (data.kind == WorkflowEditorEdgeKind.branch && data.branchIndex != null && transitions['branches'] is List) {
    final branches = asArray(transitions['branches']);

    if (data.branchIndex! >= branches.length) {
      return false;
    }

    final branch = asRecord(branches[data.branchIndex!]);

    if (nodeRefId(branch['target']) != edge.target) {
      return false;
    }

    (transitions['branches'] as List).removeAt(data.branchIndex!);
    return true;
  }

  return false;
}

bool removeWorkflowEdge(JsonRecord node, GraphEdgeLike edge) {
  if (removeEditableEdge(node, edge)) {
    return true;
  }

  final data = edge.data;

  if (data?.kind != WorkflowEditorEdgeKind.control || data?.parameterKey == null) {
    return false;
  }

  final parameters = asRecord(node['parameters']);

  if (data!.parameterIndex != null && parameters[data.parameterKey] is List) {
    final list = asArray(parameters[data.parameterKey]);

    if (data.parameterIndex! >= list.length) {
      return false;
    }

    final current = list[data.parameterIndex!];

    if (nodeRefId(current) != edge.target && nodeRefId(asRecord(current)['target']) != edge.target) {
      return false;
    }

    (parameters[data.parameterKey] as List).removeAt(data.parameterIndex!);
    return true;
  }

  if (nodeRefId(parameters[data.parameterKey]) == edge.target) {
    node['parameters'] = {
      for (final entry in parameters.entries)
        if (entry.key != data.parameterKey) entry.key: entry.value,
    };
    return true;
  }

  return false;
}

void removeWorkflowNodeReferences(JsonRecord definition, String nodeId) {
  final nodes = recordArray(definition['nodes']);

  for (final node in nodes) {
    final transitions = isRecord(node['transitions']) ? node['transitions'] as JsonRecord : <String, Object?>{};
    var currentTransitions = transitions;

    for (final key in directTransitionKeys) {
      if (nodeRefId(currentTransitions[key]) == nodeId) {
        currentTransitions = {
          for (final entry in currentTransitions.entries)
            if (entry.key != key) entry.key: entry.value,
        };
        node['transitions'] = currentTransitions;
      }
    }

    if (transitions['branches'] is List) {
      transitions['branches'] =
          recordArray(transitions['branches']).where((branch) => nodeRefId(branch['target']) != nodeId).toList();
    }

    final parameters = isRecord(node['parameters']) ? node['parameters'] as JsonRecord : <String, Object?>{};
    var currentParameters = parameters;

    for (final key in ['default', 'body', 'catch', 'finally', 'target']) {
      if (nodeRefId(currentParameters[key]) == nodeId) {
        currentParameters = {
          for (final entry in currentParameters.entries)
            if (entry.key != key) entry.key: entry.value,
        };
        node['parameters'] = currentParameters;
      }
    }

    for (final key in ['branches', 'wait_for', 'cases']) {
      if (parameters[key] is! List) {
        continue;
      }

      parameters[key] = (parameters[key] as List)
          .where((item) => nodeRefId(item) != nodeId && nodeRefId(isRecord(item) ? (item as JsonRecord)['target'] : null) != nodeId)
          .toList();
    }
  }
}

void setConditionBranch(JsonRecord node, int index, JsonRecord when, String target) {
  final transitions = asRecord(node['transitions']);
  node['transitions'] = transitions;
  final branches = asArray(transitions['branches']);
  transitions['branches'] = branches;

  final branch = {'when': when, 'target': nodeRef(target)};
  if (index < branches.length) {
    branches[index] = branch;
  } else {
    branches.add(branch);
  }
}

void removeConditionBranch(JsonRecord node, int index) {
  if (!isRecord(node['transitions']) || (node['transitions'] as JsonRecord)['branches'] is! List) {
    return;
  }

  final branches = (node['transitions'] as JsonRecord)['branches'] as List;
  if (index < branches.length) {
    branches.removeAt(index);
  }
}

WorkflowDefinition normalizeWorkflowDefinition(WorkflowDefinition workflow) {
  final definition = _normalizeDefinition(workflow.definition);
  return WorkflowDefinition(
    id: workflow.id,
    name: workflow.name,
    version: workflow.version,
    enabled: workflow.enabled,
    inputType: workflow.inputType,
    definition: definition,
    orgId: workflow.orgId,
  );
}

JsonRecord workflowLayoutNodes(JsonRecord definition) {
  final layout = asRecord(definition['ui'])['layout'];

  if (!isRecord(layout)) {
    return {};
  }

  final layoutRecord = layout as JsonRecord;

  if (isRecord(layoutRecord['nodes'])) {
    return layoutRecord['nodes'] as JsonRecord;
  }

  return layoutRecord;
}

JsonRecord _normalizeDefinition(JsonRecord definition) {
  final nextDefinition = _cloneRecord(definition);
  _normalizeLayout(nextDefinition);
  final nodes = recordArray(nextDefinition['nodes']);
  nextDefinition['nodes'] = nodes;

  final ids = nodes.map((node) => displayValue(node['id'])).where((id) => id.isNotEmpty).toSet();
  _ensureEndNode(nodes, ids);
  _ensureFailNode(nodes, ids);
  final startId = _ensureStartNode(nodes, ids);
  nextDefinition['start'] = startId;
  return nextDefinition;
}

void _normalizeLayout(JsonRecord definition) {
  final layout = asRecord(definition['ui'])['layout'];

  if (!isRecord(layout)) {
    return;
  }

  final layoutRecord = layout as JsonRecord;
  final directEntries = layoutRecord.entries.where((entry) => entry.key != 'nodes' && isRecord(entry.value)).toList();

  if (directEntries.isEmpty) {
    return;
  }

  final layoutNodes = asRecord(layoutRecord['nodes']);
  layoutRecord['nodes'] = layoutNodes;

  for (final entry in directEntries) {
    layoutNodes[entry.key] ??= entry.value;
  }

  final keysToRemove = directEntries.map((entry) => entry.key).toSet();
  final nextLayout = {
    for (final entry in layoutRecord.entries)
      if (!keysToRemove.contains(entry.key)) entry.key: entry.value,
  };
  asRecord(definition['ui'])['layout'] = nextLayout;
}

String _ensureEndNode(List<JsonRecord> nodes, Set<String> ids) {
  final existing = _firstNodeId(nodes, (kind) => kind == 'end');

  if (existing != null) {
    return existing;
  }

  final id = _uniqueNodeId('end', ids);
  nodes.add({'id': id, 'kind': 'end'});
  return id;
}

String _ensureFailNode(List<JsonRecord> nodes, Set<String> ids) {
  final existing = _firstNodeId(nodes, (kind) => kind == 'fail');

  if (existing != null) {
    return existing;
  }

  final id = _uniqueNodeId('fail', ids);
  nodes.add({'id': id, 'kind': 'fail'});
  return id;
}

String _ensureStartNode(List<JsonRecord> nodes, Set<String> ids) {
  final existing = _firstNodeId(nodes, (kind) => kind == 'start');

  if (existing != null) {
    return existing;
  }

  final id = _uniqueNodeId('start', ids);
  nodes.insert(0, {'id': id, 'kind': 'start', 'transitions': <String, Object?>{}});
  return id;
}

bool _hasSuccessTransition(JsonRecord node) {
  final transitions = node['transitions'];
  final directOk = isRecord(transitions) &&
      (nodeRefId((transitions as JsonRecord)['next']) != null ||
          nodeRefId(transitions['on_success']) != null ||
          (transitions['branches'] is List && (transitions['branches'] as List).isNotEmpty));

  return directOk || _controlFlowTargetValues(node).isNotEmpty;
}

String? _inferredNodeStatus(JsonRecord node, String id, WorkflowRunDetail? detail) {
  if (detail == null) {
    return null;
  }

  if (detail.run.activeNodeId == id && _isWorkflowRunDisplayStatus(detail.run.status)) {
    return detail.run.status;
  }

  if (node['kind'] == 'end' && detail.run.activeNodeId == id && detail.run.status == 'succeeded') {
    return 'succeeded';
  }

  if (node['kind'] == 'fail' && detail.run.activeNodeId == id && detail.run.status == 'failed') {
    return 'failed';
  }

  if (node['kind'] == 'start' && detail.nodes.isNotEmpty) {
    return 'succeeded';
  }

  return null;
}

bool _isWorkflowRunDisplayStatus(String? status) => [
      'queued',
      'running',
      'debug_paused',
      'waiting',
      'approval_required',
      'blocked',
      'succeeded',
      'failed',
      'timed_out',
      'canceled',
    ].contains(status ?? '');

String? _firstNodeId(List<JsonRecord> nodes, bool Function(String? kind) predicate) {
  final node = _firstWhereOrNull(nodes, (item) => predicate(item['kind'] is String ? item['kind'] as String : null));
  return (node != null && node['id'] != null) ? displayValue(node['id']) : null;
}

String _uniqueNodeId(String base, Set<String> ids) {
  if (!ids.contains(base)) {
    ids.add(base);
    return base;
  }

  for (var index = 2;; index += 1) {
    final candidate = '${base}_$index';

    if (!ids.contains(candidate)) {
      ids.add(candidate);
      return candidate;
    }
  }
}

JsonRecord _cloneRecord(JsonRecord value) => jsonDecode(jsonEncode(value)) as JsonRecord;

bool isRecord(Object? value) => isJsonRecord(value);

// coerce unknown json into a mutable record/array so in-place graph edits stay
// type-safe. returns the same reference when the value already matches, so
// reassigning the coerced value back onto its parent preserves mutation.
JsonRecord asRecord(Object? value) => asJsonRecord(value);

List<JsonRecord> recordArray(Object? value) {
  if (value is! List) {
    return [];
  }

  return value.whereType<JsonRecord>().toList();
}

List<JsonRecord> asArray(Object? value) => recordArray(value);

GraphEdgeModel _graphEdge(String source, String target, String label, WorkflowEditorEdgeData data) {
  final edgeLabel = (data.validationCount ?? 0) > 0 ? '$label !' : label;
  final edgeStyle = _normalizeWorkflowEdgeStyle(data.edgeStyle);
  final labelOffset = _normalizeLabelOffset(data.labelOffset);
  final labelAnchor = _normalizeLabelAnchor(data.labelAnchor);
  return GraphEdgeModel(
    id: _edgeId(source, target, label, data),
    type: 'workflow',
    source: source,
    target: target,
    sourceHandle: data.sourceHandle,
    targetHandle: data.targetHandle,
    label: edgeLabel,
    data: WorkflowEditorEdgeData(
      kind: data.kind,
      transitionKey: data.transitionKey,
      branchIndex: data.branchIndex,
      parameterKey: data.parameterKey,
      parameterIndex: data.parameterIndex,
      sourceHandle: data.sourceHandle,
      targetHandle: data.targetHandle,
      edgeStyle: edgeStyle,
      labelOffset: labelOffset,
      labelAnchor: labelAnchor,
      parallelOffset: data.parallelOffset,
      validationCount: data.validationCount,
      validationSeverity: data.validationSeverity,
      validationMessages: data.validationMessages,
      editable: data.editable,
    ),
    updatable: data.editable,
    interactionWidth: 24,
  );
}

class _LayoutEdge {
  const _LayoutEdge(this.source, this.target);

  final String source;
  final String target;
}

List<_LayoutEdge> _workflowLayoutEdges(List<JsonRecord> nodes, Set<String> nodeIds) {
  final edges = <_LayoutEdge>[];

  for (final node in nodes) {
    final source = displayValue(node['id']);

    if (source.isEmpty || !nodeIds.contains(source)) {
      continue;
    }

    final transitions = isRecord(node['transitions']) ? node['transitions'] as JsonRecord : <String, Object?>{};

    for (final key in directTransitionKeys) {
      _pushLayoutEdge(edges, source, nodeRefId(transitions[key]), nodeIds);
    }

    for (final branch in asArray(transitions['branches'])) {
      _pushLayoutEdge(edges, source, nodeRefId(asRecord(branch)['target']), nodeIds);
    }

    edges.addAll(_parameterLayoutEdges(node, source, nodeIds));
  }

  return _dedupeLayoutEdges(edges);
}

List<_LayoutEdge> _parameterLayoutEdges(JsonRecord node, String source, Set<String> nodeIds) {
  final parameters = isRecord(node['parameters']) ? node['parameters'] as JsonRecord : <String, Object?>{};
  final edges = <_LayoutEdge>[];

  switch (node['kind']) {
    case 'switch':
      for (final item in recordArray(parameters['cases'])) {
        _pushLayoutEdge(edges, source, nodeRefId(item['target']), nodeIds);
      }
      _pushLayoutEdge(edges, source, nodeRefId(parameters['default']), nodeIds);
      return edges;

    case 'toggle':
      _pushLayoutEdge(edges, source, nodeRefId(parameters['on']), nodeIds);
      _pushLayoutEdge(edges, source, nodeRefId(parameters['off']), nodeIds);
      return edges;

    case 'percentage':
      for (final item in asArray(parameters['buckets'])) {
        _pushLayoutEdge(edges, source, nodeRefId(item['target']), nodeIds);
      }
      _pushLayoutEdge(edges, source, nodeRefId(parameters['default']), nodeIds);
      return edges;

    case 'parallel':
    case 'race':
      for (final target in nodeRefArray(parameters['branches'])) {
        _pushLayoutEdge(edges, source, target, nodeIds);
      }
      return edges;

    case 'join':
      for (final dependency in nodeRefArray(parameters['wait_for'])) {
        _pushLayoutEdge(edges, dependency, source, nodeIds);
      }
      return edges;

    case 'try':
      for (final key in ['body', 'catch', 'finally']) {
        _pushLayoutEdge(edges, source, nodeRefId(parameters[key]), nodeIds);
      }
      return edges;

    case 'loop':
    case 'map':
      _pushLayoutEdge(edges, source, nodeRefId(parameters['target']), nodeIds);
      return edges;

    default:
      return edges;
  }
}

void _pushLayoutEdge(List<_LayoutEdge> edges, String source, String? target, Set<String> nodeIds) {
  if (target == null || source == target || !nodeIds.contains(source) || !nodeIds.contains(target)) {
    return;
  }

  edges.add(_LayoutEdge(source, target));
}

List<_LayoutEdge> _dedupeLayoutEdges(List<_LayoutEdge> edges) {
  final seen = <String>{};
  return edges.where((edge) {
    final key = '${edge.source} ${edge.target}';

    if (seen.contains(key)) {
      return false;
    }

    seen.add(key);
    return true;
  }).toList();
}

List<List<String>> _stronglyConnectedComponents(List<String> ids, List<_LayoutEdge> edges) {
  final adjacency = {for (final id in ids) id: <String>[]};

  for (final edge in edges) {
    adjacency[edge.source]?.add(edge.target);
  }

  final components = <List<String>>[];
  final indexById = <String, int>{};
  final lowLinkById = <String, int>{};
  final stack = <String>[];
  final onStack = <String>{};
  var nextIndex = 0;

  void visit(String id) {
    indexById[id] = nextIndex;
    lowLinkById[id] = nextIndex;
    nextIndex += 1;
    stack.add(id);
    onStack.add(id);

    for (final target in adjacency[id] ?? const <String>[]) {
      if (!indexById.containsKey(target)) {
        visit(target);
        final currentLow = lowLinkById[id];
        final targetLow = lowLinkById[target];

        if (currentLow != null && targetLow != null) {
          lowLinkById[id] = currentLow < targetLow ? currentLow : targetLow;
        }
      } else if (onStack.contains(target)) {
        final currentLow = lowLinkById[id];
        final targetIndex = indexById[target];

        if (currentLow != null && targetIndex != null) {
          lowLinkById[id] = currentLow < targetIndex ? currentLow : targetIndex;
        }
      }
    }

    if (lowLinkById[id] != indexById[id]) {
      return;
    }

    final component = <String>[];

    while (true) {
      if (stack.isEmpty) {
        break;
      }

      final member = stack.removeLast();
      onStack.remove(member);
      component.add(member);

      if (member == id) {
        break;
      }
    }

    components.add(component);
  }

  for (final id in ids) {
    if (!indexById.containsKey(id)) {
      visit(id);
    }
  }

  return components;
}

List<int> _componentLevels(
  List<List<String>> components,
  Map<int, Set<int>> componentEdges,
  Map<int, int> incomingCounts,
  Object? start,
  Map<String, int> indexById,
) {
  final levels = List.filled(components.length, 0);
  final startComponent =
      start is String ? components.indexWhere((component) => component.contains(start)) : -1;
  final queue = List<int>.generate(components.length, (i) => i).where((i) => incomingCounts[i] == 0).toList()
    ..sort((left, right) {
      if (left == startComponent) {
        return -1;
      }

      if (right == startComponent) {
        return 1;
      }

      return _componentSortKey(components[left], indexById).compareTo(_componentSortKey(components[right], indexById));
    });

  for (var i = 0; i < queue.length; i++) {
    final source = queue[i];
    for (final target in componentEdges[source] ?? const <int>{}) {
      levels[target] = levels[target] > levels[source] + 1 ? levels[target] : levels[source] + 1;
      incomingCounts[target] = (incomingCounts[target] ?? 0) - 1;

      if (incomingCounts[target] == 0) {
        queue.add(target);
      }
    }
  }

  return levels;
}

int _componentSortKey(List<String> component, Map<String, int> indexById) =>
    component.map((id) => indexById[id] ?? 0).reduce((a, b) => a < b ? a : b);

String _edgeId(String source, String target, String label, WorkflowEditorEdgeData data) {
  final parts = [
    source,
    data.kind.toJson(),
    data.transitionKey?.toJson() ?? data.parameterKey ?? data.branchIndex?.toString() ?? label,
    data.parameterIndex?.toString() ?? '',
    data.sourceHandle ?? '',
    data.targetHandle ?? '',
    _normalizeWorkflowEdgeStyle(data.edgeStyle).toJson(),
    target,
  ];
  return parts.map((part) => Uri.encodeComponent(part)).join(':');
}

List<GraphEdgeModel> _separateParallelEdges(List<GraphEdgeModel> edges) {
  final groups = <String, List<GraphEdgeModel>>{};

  for (final edge in edges) {
    final key = [edge.source, edge.target, edge.sourceHandle ?? '', edge.targetHandle ?? ''].join(' ');
    (groups[key] ??= []).add(edge);
  }

  return edges.map((edge) {
    final key = [edge.source, edge.target, edge.sourceHandle ?? '', edge.targetHandle ?? ''].join(' ');
    final group = groups[key] ?? [edge];

    if (group.length == 1) {
      return edge;
    }

    final index = group.indexWhere((item) => item.id == edge.id);
    final parallelOffset = 18 + index * 18;
    return GraphEdgeModel(
      id: edge.id,
      source: edge.source,
      target: edge.target,
      sourceHandle: edge.sourceHandle,
      targetHandle: edge.targetHandle,
      type: edge.type,
      label: edge.label,
      updatable: edge.updatable,
      interactionWidth: edge.interactionWidth,
      data: WorkflowEditorEdgeData(
        kind: edge.data.kind,
        transitionKey: edge.data.transitionKey,
        branchIndex: edge.data.branchIndex,
        parameterKey: edge.data.parameterKey,
        parameterIndex: edge.data.parameterIndex,
        sourceHandle: edge.data.sourceHandle,
        targetHandle: edge.data.targetHandle,
        edgeStyle: edge.data.edgeStyle,
        labelOffset: edge.data.labelOffset,
        labelAnchor: edge.data.labelAnchor,
        parallelOffset: parallelOffset.toDouble(),
        validationCount: edge.data.validationCount,
        validationSeverity: edge.data.validationSeverity,
        validationMessages: edge.data.validationMessages,
        editable: edge.data.editable,
      ),
      pathOptions: const GraphEdgePathOptions(offset: null, borderRadius: 8),
      zIndex: index + 1,
    );
  }).toList();
}

List<GraphEdgeModel> _controlFlowEdges(
  JsonRecord definition,
  JsonRecord node,
  Set<String> nodeIds, [
  Map<String, List<WorkflowValidationIssue>> issuesByEdge = const {},
]) {
  final source = displayValue(node['id']);
  return _controlFlowTargetValues(node).where((value) => nodeIds.contains(value.target)).map((value) {
    final semanticKey = parameterSemanticKey(value.parameterKey, value.parameterIndex);
    final issues = issuesByEdge[_edgeValidationKey(source, semanticKey)] ?? const <WorkflowValidationIssue>[];
    final handles = _edgeHandles(definition, source, semanticKey);
    return _graphEdge(
      source,
      value.target,
      value.label,
      WorkflowEditorEdgeData(
        kind: WorkflowEditorEdgeKind.control,
        parameterKey: value.parameterKey,
        parameterIndex: value.parameterIndex,
        sourceHandle: handles.sourceHandle,
        targetHandle: handles.targetHandle,
        edgeStyle: handles.edgeStyle,
        labelOffset: handles.labelOffset,
        labelAnchor: handles.labelAnchor,
        validationCount: issues.length,
        validationSeverity: _validationSeverity(issues),
        validationMessages: issues.map((issue) => issue.message).toList(),
        editable: true,
      ),
    );
  }).toList();
}

class _ControlFlowTarget {
  const _ControlFlowTarget({required this.target, required this.label, this.parameterKey, this.parameterIndex});

  final String target;
  final String label;
  final String? parameterKey;
  final int? parameterIndex;
}

List<_ControlFlowTarget> _controlFlowTargetValues(JsonRecord node) {
  final parameters = isRecord(node['parameters']) ? node['parameters'] as JsonRecord : <String, Object?>{};

  switch (node['kind']) {
    case 'switch':
      {
        final cases = recordArray(parameters['cases']);
        final targets = <_ControlFlowTarget>[];
        for (var index = 0; index < cases.length; index++) {
          final target = nodeRefId(cases[index]['target']);
          if (target != null) {
            targets.add(_ControlFlowTarget(
              target: target,
              label: displayValue(cases[index]['label']).isNotEmpty
                  ? displayValue(cases[index]['label'])
                  : 'case ${index + 1}',
              parameterKey: 'cases',
              parameterIndex: index,
            ));
          }
        }
        final defaultTarget = nodeRefId(parameters['default']);
        if (defaultTarget != null) {
          targets.add(_ControlFlowTarget(target: defaultTarget, label: 'default', parameterKey: 'default'));
        }
        return targets;
      }

    case 'toggle':
      {
        final targets = <_ControlFlowTarget>[];
        final on = nodeRefId(parameters['on']);
        if (on != null) {
          targets.add(_ControlFlowTarget(target: on, label: 'on', parameterKey: 'on'));
        }
        final off = nodeRefId(parameters['off']);
        if (off != null) {
          targets.add(_ControlFlowTarget(target: off, label: 'off', parameterKey: 'off'));
        }
        return targets;
      }

    case 'percentage':
      {
        final buckets = asArray(parameters['buckets']);
        final targets = <_ControlFlowTarget>[];
        for (var index = 0; index < buckets.length; index++) {
          final target = nodeRefId(buckets[index]['target']);
          if (target != null) {
            targets.add(_ControlFlowTarget(
              target: target,
              label: '${_jsNumber(buckets[index]['weight'] ?? 0).round()}%',
              parameterKey: 'buckets',
              parameterIndex: index,
            ));
          }
        }
        final percentageDefault = nodeRefId(parameters['default']);
        if (percentageDefault != null) {
          targets.add(_ControlFlowTarget(target: percentageDefault, label: 'default', parameterKey: 'default'));
        }
        return targets;
      }

    case 'parallel':
      {
        final branches = nodeRefArray(parameters['branches']);
        return [
          for (var index = 0; index < branches.length; index++)
            _ControlFlowTarget(target: branches[index], label: 'branch', parameterKey: 'branches', parameterIndex: index),
        ];
      }

    case 'join':
      {
        final waitFor = nodeRefArray(parameters['wait_for']);
        return [
          for (var index = 0; index < waitFor.length; index++)
            _ControlFlowTarget(target: waitFor[index], label: 'wait_for', parameterKey: 'wait_for', parameterIndex: index),
        ];
      }

    case 'try':
      {
        final targets = <_ControlFlowTarget>[];
        final body = nodeRefId(parameters['body']);
        final catchTarget = nodeRefId(parameters['catch']);
        final finallyTarget = nodeRefId(parameters['finally']);

        if (body != null) {
          targets.add(_ControlFlowTarget(target: body, label: 'body', parameterKey: 'body'));
        }
        if (catchTarget != null) {
          targets.add(_ControlFlowTarget(target: catchTarget, label: 'catch', parameterKey: 'catch'));
        }
        if (finallyTarget != null) {
          targets.add(_ControlFlowTarget(target: finallyTarget, label: 'finally', parameterKey: 'finally'));
        }
        return targets;
      }

    case 'map':
      {
        final target = nodeRefId(parameters['target']);
        return target != null ? [_ControlFlowTarget(target: target, label: 'target', parameterKey: 'target')] : [];
      }

    case 'race':
      {
        final branches = nodeRefArray(parameters['branches']);
        return [
          for (var index = 0; index < branches.length; index++)
            _ControlFlowTarget(target: branches[index], label: 'race', parameterKey: 'branches', parameterIndex: index),
        ];
      }

    default:
      return [];
  }
}

JsonRecord nodeRef(String target) => {r'$node': target};

String? nodeRefId(Object? value) {
  if (isRecord(value) && (value as JsonRecord)[r'$node'] is String && (value[r'$node'] as String).isNotEmpty) {
    return value[r'$node'] as String;
  }

  return null;
}

JsonRecord valueRef(String source, List<Object> path) => {
      r'$ref': {source: path},
    };

List<String> validateWorkflowReferenceSyntax(JsonRecord definition) => validateWorkflowIssues(definition)
    .where((issue) => issue.severity == WorkflowValidationSeverity.error)
    .map((issue) => issue.message)
    .toList();

void _pushNodeRefIssue(
  List<WorkflowValidationIssue> issues,
  Set<String> nodeIds,
  String nodeId,
  String semanticKey,
  Object? value,
  bool required,
) {
  final label = '$nodeId.$semanticKey';

  if (value == null && !required) {
    return;
  }

  final target = nodeRefId(value);

  if (target == null) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.error,
      nodeId: nodeId,
      edgeKey: _edgeValidationKey(nodeId, semanticKey),
      message: '$label must be { "\$node": "node_id" }',
    ));
    return;
  }

  if (!nodeIds.contains(target)) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.error,
      nodeId: nodeId,
      edgeKey: _edgeValidationKey(nodeId, semanticKey),
      message: '$label references missing node $target',
    ));
  }
}

void _pushControlFlowIssues(List<WorkflowValidationIssue> issues, JsonRecord node, Set<String> nodeIds, String nodeId) {
  final parameters = isRecord(node['parameters']) ? node['parameters'] as JsonRecord : <String, Object?>{};
  final kind = _workflowNodeKindOf(node['kind']);

  if (kind == 'switch') {
    final cases = recordArray(parameters['cases']);
    for (var index = 0; index < cases.length; index++) {
      _pushNodeRefIssue(issues, nodeIds, nodeId, parameterSemanticKey('cases', index), cases[index]['target'], true);
    }
    _pushNodeRefIssue(issues, nodeIds, nodeId, 'default', parameters['default'], false);
    return;
  }

  if (kind == 'toggle') {
    _pushNodeRefIssue(issues, nodeIds, nodeId, 'on', parameters['on'], true);
    _pushNodeRefIssue(issues, nodeIds, nodeId, 'off', parameters['off'], true);
    return;
  }

  if (kind == 'percentage') {
    final buckets = recordArray(parameters['buckets']);
    for (var index = 0; index < buckets.length; index++) {
      _pushNodeRefIssue(issues, nodeIds, nodeId, parameterSemanticKey('buckets', index), buckets[index]['target'], true);
    }
    _pushNodeRefIssue(issues, nodeIds, nodeId, 'default', parameters['default'], false);
    return;
  }

  if (kind == 'parallel' || kind == 'race') {
    final branches = asArray(parameters['branches']);
    for (var index = 0; index < branches.length; index++) {
      _pushNodeRefIssue(issues, nodeIds, nodeId, parameterSemanticKey('branches', index), branches[index], true);
    }
    return;
  }

  if (kind == 'join') {
    final waitFor = asArray(parameters['wait_for']);
    for (var index = 0; index < waitFor.length; index++) {
      _pushNodeRefIssue(issues, nodeIds, nodeId, parameterSemanticKey('wait_for', index), waitFor[index], true);
    }
    return;
  }

  if (kind == 'try') {
    for (final key in ['body', 'catch', 'finally']) {
      _pushNodeRefIssue(issues, nodeIds, nodeId, key, parameters[key], false);
    }
    return;
  }

  if (kind == 'loop' || kind == 'map') {
    _pushNodeRefIssue(issues, nodeIds, nodeId, 'target', parameters['target'], false);
  }
}

void _pushExpressionIssues(
  List<WorkflowValidationIssue> issues,
  Object? value,
  Set<String> nodeIds,
  String nodeId,
  String label, {
  String? edgeKey,
}) {
  if (value == null) {
    return;
  }

  if (value is String) {
    if (value.contains('{{') || value.contains('}}')) {
      issues.add(WorkflowValidationIssue(
        severity: WorkflowValidationSeverity.error,
        nodeId: nodeId,
        edgeKey: edgeKey != null ? _edgeValidationKey(nodeId, edgeKey) : null,
        message: '$label uses removed template reference syntax',
      ));
    }
    return;
  }

  if (value is List) {
    for (var index = 0; index < value.length; index++) {
      _pushExpressionIssues(issues, value[index], nodeIds, nodeId, '$label[$index]', edgeKey: edgeKey);
    }
    return;
  }

  if (!isRecord(value)) {
    return;
  }

  final record = value as JsonRecord;

  if (record.containsKey(r'$value')) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.error,
      nodeId: nodeId,
      edgeKey: edgeKey != null ? _edgeValidationKey(nodeId, edgeKey) : null,
      message: r'$label uses removed $value reference syntax'.replaceFirst(r'$label', label),
    ));
  }

  final operators = [r'$ref', r'$concat', r'$literal', r'$node'].where(record.containsKey).toList();

  if (operators.isNotEmpty && record.length != 1) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.error,
      nodeId: nodeId,
      edgeKey: edgeKey != null ? _edgeValidationKey(nodeId, edgeKey) : null,
      message: '$label expression object must contain exactly one operator',
    ));
  }

  final ref = record[r'$ref'];
  if (isRecord(ref)) {
    final refRecord = ref as JsonRecord;

    if (refRecord['node'] is String && !nodeIds.contains(refRecord['node'])) {
      issues.add(WorkflowValidationIssue(
        severity: WorkflowValidationSeverity.error,
        nodeId: nodeId,
        edgeKey: edgeKey != null ? _edgeValidationKey(nodeId, edgeKey) : null,
        message: '$label references missing node ${refRecord['node']}',
      ));
    }

    if (refRecord.containsKey('input')) {
      issues.add(WorkflowValidationIssue(
        severity: WorkflowValidationSeverity.error,
        nodeId: nodeId,
        edgeKey: edgeKey != null ? _edgeValidationKey(nodeId, edgeKey) : null,
        message: '$label uses removed input reference root',
      ));
    }

    for (final path in [refRecord['params'], refRecord['prev'], refRecord['workflow'], refRecord['output']]) {
      if (path != null && !_validRefPath(path)) {
        issues.add(WorkflowValidationIssue(
          severity: WorkflowValidationSeverity.error,
          nodeId: nodeId,
          edgeKey: edgeKey != null ? _edgeValidationKey(nodeId, edgeKey) : null,
          message: '$label has invalid reference path',
        ));
      }
    }
  }

  final concat = record[r'$concat'];
  if (concat is List) {
    for (var index = 0; index < concat.length; index++) {
      _pushExpressionIssues(issues, concat[index], nodeIds, nodeId, '$label.\$concat[$index]', edgeKey: edgeKey);
    }
  }

  if (operators.isEmpty) {
    for (final entry in record.entries) {
      _pushExpressionIssues(issues, entry.value, nodeIds, nodeId, '$label.${entry.key}', edgeKey: edgeKey);
    }
  }
}

void _pushProviderIssues(List<WorkflowValidationIssue> issues, JsonRecord node, List<ProviderMetadata> providers, String nodeId) {
  final kind = _workflowNodeKindOf(node['kind']);

  if (kind != 'action') {
    return;
  }

  final config = workflowNodeActionConfig(node);

  if (config.provider.isEmpty || config.action.isEmpty) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.warning,
      nodeId: nodeId,
      message: '$nodeId has no provider action selected',
    ));
    return;
  }

  if (providers.isEmpty) {
    return;
  }

  final provider = _firstWhereOrNull(providers, (item) => item.name == config.provider);

  if (provider == null) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.error,
      nodeId: nodeId,
      message: '$nodeId references unknown provider ${config.provider}',
    ));
    return;
  }

  final action = _firstWhereOrNull(provider.actions, (item) => item.functionName == config.action);

  if (action == null) {
    issues.add(WorkflowValidationIssue(
      severity: WorkflowValidationSeverity.error,
      nodeId: nodeId,
      message: '$nodeId references unknown action ${config.provider}.${config.action}',
    ));
    return;
  }

  final inputs = workflowNodeActionInputs(node);

  for (final parameter in action.parameters) {
    if (!parameter.required) {
      continue;
    }

    if (_isEmptyInputValue(inputs[parameter.name])) {
      issues.add(WorkflowValidationIssue(
        severity: WorkflowValidationSeverity.error,
        nodeId: nodeId,
        message: '$nodeId: ${parameter.label ?? parameter.name} is required',
      ));
    }
  }
}

bool _validRefPath(Object? value) =>
    value is List && value.every((item) => item is String || (item is int && item >= 0));

void _renameNodeRefs(Object? value, String previousId, String nextId) {
  if (value is List) {
    for (final item in value) {
      _renameNodeRefs(item, previousId, nextId);
    }
    return;
  }

  if (!isRecord(value)) {
    return;
  }

  final record = value as JsonRecord;

  if (record[r'$node'] == previousId) {
    record[r'$node'] = nextId;
  }

  for (final nested in record.values) {
    _renameNodeRefs(nested, previousId, nextId);
  }
}

Map<String, List<WorkflowValidationIssue>> _validationIssuesByNode(List<WorkflowValidationIssue> issues) {
  final map = <String, List<WorkflowValidationIssue>>{};

  for (final issue in issues) {
    (map[issue.nodeId] ??= []).add(issue);
  }

  return map;
}

Map<String, List<WorkflowValidationIssue>> _validationIssuesByEdge(List<WorkflowValidationIssue> issues) {
  final map = <String, List<WorkflowValidationIssue>>{};

  for (final issue in issues) {
    if (issue.edgeKey == null) {
      continue;
    }

    (map[issue.edgeKey!] ??= []).add(issue);
  }

  return map;
}

WorkflowValidationSeverity? _validationSeverity(List<WorkflowValidationIssue> issues) {
  if (issues.any((issue) => issue.severity == WorkflowValidationSeverity.error)) {
    return WorkflowValidationSeverity.error;
  }

  return issues.isNotEmpty ? WorkflowValidationSeverity.warning : null;
}

String _edgeValidationKey(String source, String semanticKey) => '$source:$semanticKey';

String _semanticSourceHandleId(String optionId) => 'source:${optionId.replaceAll(':', '.')}';

String? optionIdForSourceHandle(String? handleId) => (handleId != null && handleId.startsWith('source:'))
    ? handleId.substring('source:'.length).replaceAll('.', ':')
    : null;

String _workflowNodeKindOf(Object? value) {
  if (value is String && ['start', ...workflowNodeKinds, 'loop', 'end', 'fail'].contains(value)) {
    return value;
  }

  return 'action';
}

class WorkflowNodeActionConfig {
  const WorkflowNodeActionConfig({required this.provider, required this.action});

  final String provider;
  final String action;
}

WorkflowNodeActionConfig workflowNodeActionConfig(JsonRecord node) {
  final action = isRecord(node['action']) ? node['action'] as JsonRecord : <String, Object?>{};
  return WorkflowNodeActionConfig(provider: displayValue(action['provider']), action: displayValue(action['function']));
}

// effective action inputs, mirroring the runtime merge of action.configuration (base) with node.parameters (override).
JsonRecord workflowNodeActionInputs(JsonRecord node) {
  final action = isRecord(node['action']) ? node['action'] as JsonRecord : null;
  final configuration = (action != null && isRecord(action['configuration'])) ? action['configuration'] as JsonRecord : <String, Object?>{};
  final parameters = isRecord(node['parameters']) ? node['parameters'] as JsonRecord : <String, Object?>{};
  return {...configuration, ...parameters};
}

// a value sourced from another node/input counts as provided even before it resolves.
bool _isExpressionValue(Object? value) {
  if (!isRecord(value)) {
    return false;
  }

  final record = value as JsonRecord;
  return [r'$ref', r'$concat', r'$coalesce', r'$literal', r'$to_string', r'$to_json_string', r'$node']
      .any(record.containsKey);
}

bool _isEmptyInputValue(Object? value) {
  if (_isExpressionValue(value)) {
    return false;
  }

  return isBlankValue(value);
}

List<ActionResultMetadata> workflowNodeResultMetadata(JsonRecord node, List<ProviderMetadata> providers) {
  final config = workflowNodeActionConfig(node);

  if (config.provider.isEmpty || config.action.isEmpty) {
    return [];
  }

  final provider = _firstWhereOrNull(providers, (item) => item.name == config.provider);
  final action = provider != null ? _firstWhereOrNull(provider.actions, (item) => item.functionName == config.action) : null;
  return action?.results ?? [];
}

// the title shown on the node, falling back to the id when no custom name is set.
String _nodeDisplayName(JsonRecord node, String id) {
  final name = node['name'] is String ? (node['name'] as String).trim() : '';
  return name.isNotEmpty ? name : id;
}

// renders any value (string, ref, expression, object) into a short human-readable label.
String _describeValue(Object? value) {
  if (value == null) {
    return '';
  }

  if (value is String) {
    return value;
  }

  if (value is num || value is bool) {
    return value.toString();
  }

  if (isRecord(value)) {
    final record = value as JsonRecord;

    if (record[r'$node'] is String) {
      return '→ ${record[r'$node']}';
    }

    if (isRecord(record[r'$ref'])) {
      final refEntries = (record[r'$ref'] as JsonRecord).entries;
      final first = refEntries.isNotEmpty ? refEntries.first : null;
      final source = first?.key ?? '';
      final path = first?.value;
      final segments = path is List ? path.join('.') : '';
      return r'${' '$source${segments.isNotEmpty ? '.$segments' : ''}}'.replaceFirst(r'$ ', r'$');
    }

    if (record.containsKey(r'$value')) {
      return _describeValue(record[r'$value']);
    }
  }

  if (value is List) {
    return value.isEmpty ? '[]' : '[${value.length} item${value.length == 1 ? '' : 's'}]';
  }

  try {
    final json = jsonEncode(value);
    return json.length > 60 ? '${json.substring(0, 57)}…' : json;
  } catch (_) {
    return '…';
  }
}

// each node kind renders a concise, never-"[object Object]" description of its activity.
String _nodeSummary(JsonRecord node, Map<String, String>? subflowNames) {
  final parameters = asRecord(node['parameters']);

  switch (_workflowNodeKindOf(node['kind'])) {
    case 'action':
      {
        final config = workflowNodeActionConfig(node);

        if (config.provider.isEmpty) {
          return 'Unconfigured action';
        }

        return config.action.isNotEmpty ? '${config.provider}.${config.action}' : config.provider;
      }

    case 'approval':
      return _describeValue(parameters['prompt']).isNotEmpty ? _describeValue(parameters['prompt']) : 'Approval required';

    case 'condition':
      {
        final branches = asArray(asRecord(node['transitions'])['branches']);
        final count = branches.length;
        return '$count branch${count == 1 ? '' : 'es'}';
      }

    case 'switch':
      {
        final count = parameters['cases'] is List ? (parameters['cases'] as List).length : 0;
        final value = _describeValue(parameters['value']);
        return 'Switch on ${value.isNotEmpty ? value : 'value'} ($count case${count == 1 ? '' : 's'})';
      }

    case 'toggle':
      {
        final value = _describeValue(parameters['value']);
        return 'Toggle on ${value.isNotEmpty ? value : 'value'}';
      }

    case 'percentage':
      {
        final count = parameters['buckets'] is List ? (parameters['buckets'] as List).length : 0;
        final key = _describeValue(parameters['key']);
        return 'Split on ${key.isNotEmpty ? key : 'key'} ($count bucket${count == 1 ? '' : 's'})';
      }

    case 'wait':
      {
        final wait = asRecord(node['wait']);
        final until = wait['until_status'];

        if (until != null) {
          return 'Wait for ${_describeValue(until)}';
        }

        final seconds = _jsNumber(wait['seconds'] ?? 0);
        return seconds > 0 ? 'Wait ${seconds.toInt()}s' : 'Wait';
      }

    case 'loop':
      {
        final target = nodeRefId(parameters['target']);
        final max = _jsNumber(node['max_iterations'] ?? 0).toInt();
        return 'Loop${target != null ? ' → $target' : ''}${max != 0 ? ' ×$max' : ''}';
      }

    case 'map':
      {
        final target = nodeRefId(parameters['target']);
        final concurrency = _jsNumber(parameters['concurrency'] ?? 1).toInt();
        return 'Map${target != null ? ' → $target' : ''} (×$concurrency)';
      }

    case 'parallel':
      {
        final count = nodeRefArray(parameters['branches']).length;
        return '$count parallel branch${count == 1 ? '' : 'es'}';
      }

    case 'race':
      {
        final count = nodeRefArray(parameters['branches']).length;
        return 'Race $count branch${count == 1 ? '' : 'es'}';
      }

    case 'join':
      {
        final count = nodeRefArray(parameters['wait_for']).length;
        final mode = _describeValue(parameters['mode']);
        return 'Join $count (${mode.isNotEmpty ? mode : 'all'})';
      }

    case 'try':
      {
        final parts = ['body', 'catch', 'finally'].where((key) => nodeRefId(parameters[key]) != null).toList();
        return parts.isNotEmpty ? 'Try (${parts.join(", ")})' : 'Try';
      }

    case 'output':
      {
        final eventType = _describeValue(parameters['event_type']);
        return 'Output ${eventType.isNotEmpty ? eventType : 'workflow.output'}';
      }

    case 'input':
      {
        final prompt = _describeValue(parameters['prompt']);
        return prompt.isNotEmpty ? prompt : 'Input';
      }

    case 'config':
      {
        final name = _describeValue(parameters['name']);
        return name.isNotEmpty ? name : 'Config';
      }

    case 'subflow':
      {
        final subflowId = node['subflow_id'] != null ? displayValue(node['subflow_id']) : '';
        final name = subflowId.isNotEmpty ? (subflowNames?[subflowId]) : null;

        if (name != null) {
          return name;
        }

        return 'Workflow ${subflowId.isNotEmpty ? subflowId : '-'}';
      }

    case 'start':
      return 'Start';
    case 'end':
      return 'Success';
    case 'fail':
      return 'Workflow failure';
    default:
      return _workflowNodeKindOf(node['kind']);
  }
}

String? _approvalPrompt(JsonRecord node, JsonRecord? state) {
  if (_workflowNodeKindOf(node['kind']) != 'approval') {
    return null;
  }

  final value = state?['prompt'] ?? asRecord(state?['approval'])['prompt'] ?? asRecord(node['parameters'])['prompt'];
  final described = _describeValue(value);
  return described.isNotEmpty ? described : 'Approval required';
}

String? _inputPrompt(JsonRecord node, JsonRecord? state) {
  if (_workflowNodeKindOf(node['kind']) != 'input') {
    return null;
  }

  final value = asRecord(state?['input'])['prompt'] ?? asRecord(node['parameters'])['prompt'];
  final described = _describeValue(value);
  return described.isNotEmpty ? described : 'Input required';
}

String _firstAvailableTransition(JsonRecord node) {
  final transitions = isRecord(node['transitions']) ? node['transitions'] as JsonRecord : <String, Object?>{};
  return _firstWhereOrNull(directTransitionKeys, (key) => transitions[key] == null) ?? 'next';
}

class _EdgeHandlePair {
  const _EdgeHandlePair({this.sourceHandle, this.targetHandle, this.edgeStyle, this.labelOffset, this.labelAnchor});

  final String? sourceHandle;
  final String? targetHandle;
  final WorkflowEdgeStyle? edgeStyle;
  final WorkflowEdgeLabelOffset? labelOffset;
  final WorkflowEdgeLabelAnchor? labelAnchor;
}

_EdgeHandlePair _edgeHandles(JsonRecord definition, String source, String semanticKey) {
  final edgeHandleMap = asRecord(asRecord(definition['ui'])['edge_handles']);
  final saved = asRecord(edgeHandleMap[_edgeHandleKey(source, semanticKey)]);
  return _EdgeHandlePair(
    sourceHandle: _normalizeConnectionHandle(saved['sourceHandle']) ?? _semanticSourceHandleId(_optionIdFromSemanticKey(semanticKey)),
    targetHandle: _normalizeConnectionHandle(saved['targetHandle']) ?? _semanticTargetHandleId,
    edgeStyle: _normalizeWorkflowEdgeStyle(saved['edgeStyle'] is String ? WorkflowEdgeStyle.fromJson(saved['edgeStyle'] as String) : null),
    labelOffset: _normalizeLabelOffset(saved['labelOffset']),
    labelAnchor: _normalizeLabelAnchor(saved['labelAnchor']),
  );
}

_EdgeHandlePair _connectionHandlesForPositions(WorkflowLayoutPosition? source, WorkflowLayoutPosition? target) {
  if (source == null || target == null) {
    return const _EdgeHandlePair(sourceHandle: 'bottom', targetHandle: 'top');
  }

  final dx = target.x - source.x;
  final dy = target.y - source.y;

  if (dx.abs() >= dy.abs()) {
    return dx >= 0
        ? const _EdgeHandlePair(sourceHandle: 'right', targetHandle: 'left')
        : const _EdgeHandlePair(sourceHandle: 'left', targetHandle: 'right');
  }

  return dy >= 0
      ? const _EdgeHandlePair(sourceHandle: 'bottom', targetHandle: 'top')
      : const _EdgeHandlePair(sourceHandle: 'top', targetHandle: 'bottom');
}

String parameterSemanticKey(String? parameterKey, [int? parameterIndex]) {
  if (parameterIndex != null) {
    return '${parameterKey ?? 'control'}.$parameterIndex';
  }

  return parameterKey ?? 'control';
}

String _edgeHandleKey(String source, String semanticKey) => '$source:$semanticKey';

String _optionIdFromSemanticKey(String semanticKey) {
  if (directTransitionKeys.contains(semanticKey)) {
    return 'direct:$semanticKey';
  }

  if (semanticKey.startsWith('branches.')) {
    return 'branch:${semanticKey.substring('branches.'.length)}';
  }

  if (semanticKey.contains('.')) {
    final parts = semanticKey.split('.');
    return 'control:${parts[0]}:${parts[1]}';
  }

  return 'control:$semanticKey';
}

String titleCase(String value) => value
    .split(RegExp(r'[_\s-]+'))
    .where((part) => part.isNotEmpty)
    .map((part) => part[0].toUpperCase() + part.substring(1))
    .join(' ');

String _transitionLabel(String value) => titleCase(value.replaceFirst(RegExp(r'^on_'), ''));

String? _normalizeConnectionHandle(Object? value) => (value is String && value.isNotEmpty) ? value : null;

WorkflowEdgeStyle _normalizeWorkflowEdgeStyle(WorkflowEdgeStyle? value) => value ?? WorkflowEdgeStyle.square;

WorkflowEdgeLabelOffset? _normalizeLabelOffset(Object? value) {
  if (value is WorkflowEdgeLabelOffset) {
    if (value.x == 0 && value.y == 0) {
      return null;
    }
    return value;
  }

  if (!isRecord(value)) {
    return null;
  }

  final record = value as JsonRecord;
  final x = _jsNumber(record['x']);
  final y = _jsNumber(record['y']);

  if (x.isNaN || y.isNaN) {
    return null;
  }

  if (x == 0 && y == 0) {
    return null;
  }

  return WorkflowEdgeLabelOffset(x: x, y: y);
}

WorkflowEdgeLabelAnchor? _normalizeLabelAnchor(Object? value) {
  double? position;

  if (value is WorkflowEdgeLabelAnchor) {
    position = value.position;
  } else if (isRecord(value)) {
    position = _jsNumber((value as JsonRecord)['position']);
  } else {
    return null;
  }

  if (position.isNaN) {
    return null;
  }

  final clamped = position.clamp(0, 1).toDouble();

  if ((clamped - 0.5).abs() < 0.001) {
    return null;
  }

  return WorkflowEdgeLabelAnchor(position: clamped);
}
