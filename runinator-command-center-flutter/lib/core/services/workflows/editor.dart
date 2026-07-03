// port of core/services/workflows/editor.ts.

import 'dart:async';
import 'dart:convert';

import '../../api/command_center_api.dart' as api;
import '../../domain/json.dart';
import '../../domain/models/index.dart';
import '../../utils/format.dart' show pretty, errorMessage;
import '../../utils/json_utils.dart' show parseRequiredObject, parseRequiredJson;
import '../../utils/values.dart' show displayValue, isBlankValue;
import '../../workflow/editor_defaults.dart';
import '../../workflow/graph_model.dart';
import '../../workflow/workflow_helpers.dart';
import 'catalog.dart' show WorkflowEditorPeer;
import 'host.dart';
import 'state.dart';

const int _workflowWdlSyncDelayMs = 1500;

abstract class WorkflowCatalogPeer {
  Future<void> saveSelectedWorkflowBundle();
}

abstract class WorkflowRunsEditorPeer {
  Future<void> updateSelectedWorkflowNodeDetail();
}

class _StepJsonResult {
  const _StepJsonResult.ok(this.value) : ok = true;

  const _StepJsonResult.error()
      : ok = false,
        value = null;

  final bool ok;
  final JsonValue value;
}

class _OptionalExprResult {
  const _OptionalExprResult.ok(this.value)
      : ok = true,
        present = true;

  const _OptionalExprResult.error()
      : ok = false,
        value = null,
        present = false;

  const _OptionalExprResult.absent()
      : ok = true,
        value = null,
        present = false;

  final bool ok;
  final JsonValue value;
  final bool present;
}

class WorkflowEditorService implements WorkflowEditorPeer {
  WorkflowEditorService(this._host, this._runs, this._catalog);

  final WorkflowServiceHost _host;
  final WorkflowRunsEditorPeer _runs;
  final WorkflowCatalogPeer _catalog;

  void addWorkflowStep() => addWorkflowNode('action');

  void addWorkflowNode(String kind) {
    final nodes = ensureWorkflowNodes();
    final newNode = createWorkflowNode(kind, nodes);
    _stripNewNodeConnections(newNode);
    final position = _graphCentroidPosition();
    final endIndex = nodes.indexWhere((node) => node['kind'] == 'end');

    if (endIndex >= 0) {
      nodes.insert(endIndex, newNode);
    } else {
      nodes.add(newNode);
    }

    _setGraphNodePosition(displayValue(newNode['id']), position);
    syncWorkflowDraftToJson();
    populateStepEditor(displayValue(newNode['id']));
    openStepEditor(displayValue(newNode['id']), creating: true);
  }

  void addConnectedWorkflowNode([String kind = 'action']) => addWorkflowNode(kind);

  void removeWorkflowStep() {
    if (_host.state.selectedStepId.isEmpty || !_host.canRemoveSelectedStep()) {
      return;
    }

    removeWorkflowNode(_host.state.selectedStepId);
  }

  void removeWorkflowNode(String nodeId) {
    final nodes = ensureWorkflowNodes();
    final match = nodes.where((item) => item['id'] == nodeId);
    final node = match.isNotEmpty ? match.first : null;

    if (node == null || isLockedWorkflowNode(node)) {
      return;
    }

    _host.state.workflowDraft.definition['nodes'] = ensureWorkflowNodes().where((item) => item['id'] != nodeId).toList();
    removeWorkflowNodeReferences(_host.state.workflowDraft.definition, nodeId);
    final layout = asRecord(asRecord(_host.state.workflowDraft.definition['ui'])['layout']);
    final layoutNodes = asRecord(layout['nodes']);
    layout['nodes'] = {
      for (final entry in layoutNodes.entries)
        if (entry.key != nodeId) entry.key: entry.value,
    };

    if (_host.state.selectedStepId == nodeId) {
      _host.state.selectedStepId = '';
    }

    syncWorkflowDraftToJson();
  }

  bool applyInlineNodeEdit(String nodeId, String nextId, String inlineValue) {
    final previousId = nodeId;
    final result = applyWorkflowInlineNodeEdit(_host.state.workflowDraft.definition, nodeId, nextId, inlineValue);

    if (!result.ok) {
      _host.setError(result.message!);
      return false;
    }

    if (previousId != result.nodeId) {
      _renameLayoutNode(previousId, result.nodeId!);
    }

    _host.state.selectedStepId = result.nodeId!;
    syncWorkflowDraftToJson();
    populateStepEditor(result.nodeId!);
    return true;
  }

  void clearWorkflowGraphSelection() {
    _host.state.selectedStepId = '';
    _host.state.inlineEditNodeId = '';
    _host.state.selectedGraphEdgeId = '';
  }

  bool submitInlineNodeEdit(String nodeId, String nextId, String inlineValue) {
    if (!applyInlineNodeEdit(nodeId, nextId, inlineValue)) {
      return false;
    }

    clearWorkflowGraphSelection();
    return true;
  }

  bool applyStepEditor() {
    _host.internal.stepEditorApplyTimer?.cancel();
    _host.internal.stepEditorApplyTimer = null;
    _host.state.stepEditorError = '';

    if (_host.state.selectedStepId.isEmpty) {
      return false;
    }

    final nodes = ensureWorkflowNodes();
    final index = nodes.indexWhere((node) => node['id'] == _host.state.selectedStepId);

    if (index < 0) {
      return false;
    }

    if (isLockedWorkflowNode(nodes[index]) && _host.state.stepEditor.kind != nodes[index]['kind']) {
      final message = '${nodes[index]['kind']} node kind cannot be changed';
      _host.state.stepEditorError = message;
      _host.setError(message);
      return false;
    }

    final parameters = parseRequiredObject(_host.state.stepEditor.parametersJson);
    final transitions = parseRequiredObject(_host.state.stepEditor.transitionsJson);

    if (parameters == null || transitions == null) {
      final message = parameters != null ? 'Node transitions must be a JSON object' : 'Step parameters must be a JSON object';
      _host.state.stepEditorError = message;
      _host.setError(message);
      return false;
    }

    final parameterError = _validateStepParameters(parameters);

    if (parameterError.isNotEmpty) {
      _host.state.stepEditorError = parameterError;
      _host.setError(parameterError);
      return false;
    }

    final next = <String, Object?>{...nodes[index]};
    next['id'] = _host.state.stepEditor.id.trim();

    if ((next['id'] as String).isEmpty) {
      _host.state.stepEditorError = 'Step ID is required';
      return false;
    }

    final trimmedName = _host.state.stepEditor.name.trim();

    if (trimmedName.isNotEmpty) {
      next['name'] = trimmedName;
    } else {
      next.remove('name');
    }

    next['kind'] = _host.state.stepEditor.kind;

    if (next['kind'] == 'action') {
      final previousAction = isRecord(next['action']) ? next['action'] as JsonRecord : <String, Object?>{};
      next['action'] = {
        ...previousAction,
        'provider': _host.state.stepEditor.actionName,
        'function': _host.state.stepEditor.actionFunction,
        'timeout_seconds': _host.state.stepEditor.timeoutSeconds > 0 ? _host.state.stepEditor.timeoutSeconds : (previousAction['timeout_seconds'] ?? 300),
        'configuration': parameters,
      };
    } else {
      next.remove('action');
    }

    next['retry'] = {'max_attempts': _host.state.stepEditor.maxAttempts};

    if (_host.state.stepEditor.timeoutSeconds > 0) {
      next['timeout_seconds'] = _host.state.stepEditor.timeoutSeconds;
    } else {
      next.remove('timeout_seconds');
    }

    if (isProtectedWorkflowNode(next)) {
      next.remove('locked');
    } else if (_host.state.stepEditor.locked) {
      next['locked'] = true;
    } else {
      next.remove('locked');
    }

    if (_host.state.stepEditor.skipped) {
      next['skipped'] = true;
    } else {
      next.remove('skipped');
    }

    // action nodes store inputs in action.configuration (set above); keep node.parameters clear to avoid duplication.
    next['parameters'] = next['kind'] == 'action' ? <String, Object?>{} : parameters;
    next['transitions'] = transitions;

    if (next['kind'] == 'approval') {
      next['parameters'] = {
        ...parameters,
        'approval_type': _host.state.stepEditor.approvalType.isNotEmpty ? _host.state.stepEditor.approvalType : 'generic',
        'prompt': _host.state.stepEditor.approvalPrompt.isNotEmpty ? _host.state.stepEditor.approvalPrompt : 'Approval required',
      };
    }

    if (next['kind'] == 'gate') {
      final gateParams = <String, Object?>{...parameters, 'kind': _host.state.stepEditor.gateKind.isNotEmpty ? _host.state.stepEditor.gateKind : 'manual'};

      if (_host.state.stepEditor.gateKind == 'condition') {
        final when = parseRequiredObject(_host.state.stepEditor.gateWhenJson);

        if (when == null) {
          _host.state.stepEditorError = 'Gate condition must be a JSON object';
          _host.setError(_host.state.stepEditorError);
          return false;
        }

        gateParams['when'] = when;
      } else {
        gateParams.remove('when');
      }

      final pollInterval = _host.state.stepEditor.gatePollInterval;

      if (pollInterval > 0) {
        gateParams['poll_interval'] = pollInterval;
      } else {
        gateParams.remove('poll_interval');
      }

      final timeout = _host.state.stepEditor.gateTimeout;

      if (timeout > 0) {
        gateParams['timeout'] = timeout;
      } else {
        gateParams.remove('timeout');
      }

      if (_host.state.stepEditor.gateLabel.trim().isNotEmpty) {
        gateParams['label'] = _host.state.stepEditor.gateLabel.trim();
      } else {
        gateParams.remove('label');
      }

      next['parameters'] = gateParams;
    }

    if (next['kind'] == 'signal') {
      next['parameters'] = {...parameters, 'name': _host.state.stepEditor.signalName.trim().isNotEmpty ? _host.state.stepEditor.signalName.trim() : 'signal'};
    }

    if (next['kind'] == 'condition') {
      final conditionTransitions = <String, Object?>{...transitions, 'branches': <Object?>[]};
      next['transitions'] = conditionTransitions;

      for (var branchIndex = 0; branchIndex < _host.state.stepEditor.conditionBranches.length; branchIndex++) {
        final branch = _host.state.stepEditor.conditionBranches[branchIndex];
        final when = parseRequiredObject(branch.whenJson);

        if (when == null) {
          _host.state.stepEditorError = 'Condition branch ${branchIndex + 1} must be a JSON object';
          _host.setError(_host.state.stepEditorError);
          return false;
        }

        if (branch.target.isEmpty) {
          _host.state.stepEditorError = 'Condition branch ${branchIndex + 1} needs a target';
          _host.setError(_host.state.stepEditorError);
          return false;
        }

        setConditionBranch(next, branchIndex, when, branch.target);
      }

      if (_host.state.stepEditor.conditionFallback.isNotEmpty) {
        conditionTransitions['next'] = nodeRef(_host.state.stepEditor.conditionFallback);
      } else {
        conditionTransitions.remove('next');
      }
    }

    if (next['kind'] == 'wait') {
      final wait = parseRequiredObject(_host.state.stepEditor.waitJson);

      if (wait == null) {
        _host.state.stepEditorError = 'Wait settings must be a JSON object';
        _host.setError(_host.state.stepEditorError);
        return false;
      }

      final waitNext = <String, Object?>{...wait, 'seconds': _host.state.stepEditor.waitSeconds < 0 ? 0 : _host.state.stepEditor.waitSeconds};

      if (_host.state.stepEditor.waitInitialStatus.trim().isNotEmpty) {
        waitNext['initial_status'] = _host.state.stepEditor.waitInitialStatus.trim();
      } else {
        waitNext.remove('initial_status');
      }

      if (_host.state.stepEditor.waitUntilStatus.trim().isNotEmpty) {
        waitNext['until_status'] = _host.state.stepEditor.waitUntilStatus.trim();
      } else {
        waitNext.remove('until_status');
      }

      next['wait'] = waitNext;
    } else {
      next.remove('wait');
    }

    if (next['kind'] == 'loop') {
      final items = _parseStepJson('Loop items', _host.state.stepEditor.loopItemsJson);

      if (!items.ok) {
        return false;
      }

      final loopParams = <String, Object?>{...parameters, 'items': items.value};

      if (_host.state.stepEditor.loopTarget.isNotEmpty) {
        loopParams['target'] = nodeRef(_host.state.stepEditor.loopTarget);
      } else {
        loopParams.remove('target');
      }

      next['parameters'] = loopParams;
      next['max_iterations'] = _host.state.stepEditor.loopMaxIterations < 1 ? 1 : _host.state.stepEditor.loopMaxIterations;
    } else {
      next.remove('max_iterations');
    }

    if (next['kind'] == 'switch') {
      final value = _parseStepJson('Switch value', _host.state.stepEditor.switchValueJson);

      if (!value.ok) {
        return false;
      }

      final cases = <JsonRecord>[];

      for (var caseIndex = 0; caseIndex < _host.state.stepEditor.switchCases.length; caseIndex++) {
        final switchCase = _host.state.stepEditor.switchCases[caseIndex];

        if (switchCase.target.isEmpty) {
          _setStepEditorError('Switch case ${caseIndex + 1} needs a target');
          return false;
        }

        final match = _parseStepJson('Switch case ${caseIndex + 1}', switchCase.matchJson);

        if (!match.ok) {
          return false;
        }

        final serialized = <String, Object?>{'target': nodeRef(switchCase.target)};

        if (switchCase.matchKind == 'when') {
          serialized['when'] = match.value;
        } else if (switchCase.matchKind == 'exists') {
          serialized['exists'] = match.value == true;
        } else {
          serialized[switchCase.matchKind] = match.value;
        }

        cases.add(serialized);
      }

      final switchParams = <String, Object?>{...parameters, 'value': value.value, 'cases': cases};

      if (_host.state.stepEditor.switchDefault.isNotEmpty) {
        switchParams['default'] = nodeRef(_host.state.stepEditor.switchDefault);
      } else {
        switchParams.remove('default');
      }

      next['parameters'] = switchParams;
    }

    if (next['kind'] == 'toggle') {
      final value = _parseStepJson('Toggle value', _host.state.stepEditor.toggleValueJson);

      if (!value.ok) {
        return false;
      }

      if (_host.state.stepEditor.toggleOn.isEmpty || _host.state.stepEditor.toggleOff.isEmpty) {
        _setStepEditorError('Toggle needs both an on and an off target');
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'value': value.value,
        'on': nodeRef(_host.state.stepEditor.toggleOn),
        'off': nodeRef(_host.state.stepEditor.toggleOff),
      };
    }

    if (next['kind'] == 'percentage') {
      final key = _parseStepJson('Percentage key', _host.state.stepEditor.percentageKeyJson);

      if (!key.ok) {
        return false;
      }

      final buckets = <JsonRecord>[];

      for (var bucketIndex = 0; bucketIndex < _host.state.stepEditor.percentageBuckets.length; bucketIndex++) {
        final bucket = _host.state.stepEditor.percentageBuckets[bucketIndex];

        if (bucket.target.isEmpty) {
          _setStepEditorError('Bucket ${bucketIndex + 1} needs a target');
          return false;
        }

        final weight = bucket.weight.truncate();

        if (weight <= 0) {
          _setStepEditorError('Bucket ${bucketIndex + 1} needs a weight greater than zero');
          return false;
        }

        buckets.add({'weight': weight, 'target': nodeRef(bucket.target)});
      }

      final percentageParams = <String, Object?>{...parameters, 'key': key.value, 'buckets': buckets};

      if (_host.state.stepEditor.percentageDefault.isNotEmpty) {
        percentageParams['default'] = nodeRef(_host.state.stepEditor.percentageDefault);
      } else {
        percentageParams.remove('default');
      }

      next['parameters'] = percentageParams;
    }

    if (next['kind'] == 'parallel') {
      next['parameters'] = {
        ...parameters,
        'branches': _host.state.stepEditor.parallelBranches.where((b) => b.isNotEmpty).map(nodeRef).toList(),
      };
    }

    if (next['kind'] == 'join') {
      next['parameters'] = {
        ...parameters,
        'wait_for': _host.state.stepEditor.joinWaitFor.where((b) => b.isNotEmpty).map(nodeRef).toList(),
        'mode': _host.state.stepEditor.joinMode,
      };
    }

    if (next['kind'] == 'try') {
      final tryParams = <String, Object?>{...parameters};

      if (_host.state.stepEditor.tryBody.isNotEmpty) {
        tryParams['body'] = nodeRef(_host.state.stepEditor.tryBody);
      } else {
        tryParams.remove('body');
      }

      if (_host.state.stepEditor.tryCatch.isNotEmpty) {
        tryParams['catch'] = nodeRef(_host.state.stepEditor.tryCatch);
      } else {
        tryParams.remove('catch');
      }

      if (_host.state.stepEditor.tryFinally.isNotEmpty) {
        tryParams['finally'] = nodeRef(_host.state.stepEditor.tryFinally);
      } else {
        tryParams.remove('finally');
      }

      next['parameters'] = tryParams;
    }

    if (next['kind'] == 'map') {
      final items = _parseStepJson('Map items', _host.state.stepEditor.mapItemsJson);

      if (!items.ok) {
        return false;
      }

      final mapParams = <String, Object?>{
        ...parameters,
        'items': items.value,
        'concurrency': _host.state.stepEditor.mapConcurrency < 1 ? 1 : _host.state.stepEditor.mapConcurrency,
      };

      if (_host.state.stepEditor.mapTarget.isNotEmpty) {
        mapParams['target'] = nodeRef(_host.state.stepEditor.mapTarget);
      } else {
        mapParams.remove('target');
      }

      next['parameters'] = mapParams;
    }

    if (next['kind'] == 'race') {
      next['parameters'] = {
        ...parameters,
        'branches': _host.state.stepEditor.raceBranches.where((b) => b.isNotEmpty).map(nodeRef).toList(),
        'winner': _host.state.stepEditor.raceWinner,
      };
    }

    if (next['kind'] == 'output') {
      final data = _parseStepJson('Output data', _host.state.stepEditor.outputDataJson);

      if (!data.ok) {
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'event_type': _host.state.stepEditor.outputEventType.trim().isNotEmpty ? _host.state.stepEditor.outputEventType.trim() : 'workflow.output',
        'data': data.value,
      };
    }

    if (next['kind'] == 'input') {
      next['parameters'] = {
        ...parameters,
        'prompt': _host.state.stepEditor.inputPrompt.trim().isNotEmpty ? _host.state.stepEditor.inputPrompt.trim() : 'Provide input',
      };
    }

    if (next['kind'] == 'config') {
      final name = _parseStepJson('Config name', _host.state.stepEditor.configNameJson);

      if (!name.ok) {
        return false;
      }

      final metadata = _parseStepJson('Config metadata', _host.state.stepEditor.configMetadataJson);

      if (!metadata.ok) {
        return false;
      }

      next['parameters'] = {...parameters, 'name': name.value, 'metadata': metadata.value};
    }

    if (next['kind'] == 'subflow') {
      final subflowParameters = parseRequiredObject(_host.state.stepEditor.subflowParametersJson);

      if (subflowParameters == null) {
        _setStepEditorError('Subflow parameters must be a JSON object');
        return false;
      }

      if (_host.state.stepEditor.subflowId.trim().isEmpty) {
        _setStepEditorError('Subflow workflow id is required');
        return false;
      }

      next['subflow_id'] = _host.state.stepEditor.subflowId.trim();
      next['parameters'] = subflowParameters;
    } else {
      next.remove('subflow_id');
    }

    if (next['kind'] == 'assert') {
      final assertions = <JsonRecord>[];

      for (var assertIndex = 0; assertIndex < _host.state.stepEditor.assertAssertions.length; assertIndex++) {
        final assertion = _host.state.stepEditor.assertAssertions[assertIndex];
        final condition = _parseStepJson('Assertion ${assertIndex + 1} condition', assertion.conditionJson);

        if (!condition.ok) {
          return false;
        }

        final serialized = <String, Object?>{'condition': condition.value};

        if (assertion.name.trim().isNotEmpty) {
          serialized['name'] = assertion.name.trim();
        }

        if (assertion.message.trim().isNotEmpty) {
          serialized['message'] = assertion.message.trim();
        }

        assertions.add(serialized);
      }

      next['parameters'] = {...parameters, 'assertions': assertions};
    }

    if (next['kind'] == 'transform') {
      final bindings = parseRequiredObject(_host.state.stepEditor.transformBindingsJson);

      if (bindings == null) {
        _setStepEditorError('Transform bindings must be a JSON object');
        return false;
      }

      next['parameters'] = {...parameters, 'bindings': bindings};
    }

    if (next['kind'] == 'audit') {
      final action = _parseStepJson('Audit action', _host.state.stepEditor.auditActionJson);

      if (!action.ok) {
        return false;
      }

      final auditParams = <String, Object?>{...parameters, 'action': action.value};
      final optionalAudit = <String, Object?>{};

      for (final entry in [
        ('actor', _host.state.stepEditor.auditActorJson),
        ('target', _host.state.stepEditor.auditTargetJson),
        ('reason', _host.state.stepEditor.auditReasonJson),
      ]) {
        final parsed = _parseOptionalExpr('Audit ${entry.$1}', entry.$2);

        if (!parsed.ok) {
          return false;
        }

        if (parsed.present) {
          optionalAudit[entry.$1] = parsed.value;
        }
      }

      next['parameters'] = {...auditParams, ...optionalAudit};
    }

    if (next['kind'] == 'checkpoint') {
      if (_host.state.stepEditor.checkpointName.trim().isEmpty) {
        _setStepEditorError('Checkpoint needs a name');
        return false;
      }

      next['parameters'] = {...parameters, 'name': _host.state.stepEditor.checkpointName.trim()};
    }

    if (next['kind'] == 'mutex') {
      if (_host.state.stepEditor.mutexName.trim().isEmpty) {
        _setStepEditorError('Mutex needs a name');
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'name': _host.state.stepEditor.mutexName.trim(),
        'poll_interval_seconds': _host.state.stepEditor.mutexPollInterval < 1 ? 1 : _host.state.stepEditor.mutexPollInterval,
      };
    }

    if (next['kind'] == 'throttle') {
      if (_host.state.stepEditor.throttleName.trim().isEmpty) {
        _setStepEditorError('Throttle needs a name');
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'name': _host.state.stepEditor.throttleName.trim(),
        'max_per_window': _host.state.stepEditor.throttleMaxPerWindow < 1 ? 1 : _host.state.stepEditor.throttleMaxPerWindow,
        'window_seconds': _host.state.stepEditor.throttleWindowSeconds < 1 ? 1 : _host.state.stepEditor.throttleWindowSeconds,
        'poll_interval_seconds': _host.state.stepEditor.throttlePollInterval < 1 ? 1 : _host.state.stepEditor.throttlePollInterval,
      };
    }

    if (next['kind'] == 'await_run') {
      final runIds = _parseStepJson('Await run ids', _host.state.stepEditor.awaitRunIdsJson);

      if (!runIds.ok) {
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'run_ids': runIds.value,
        'mode': _host.state.stepEditor.awaitMode == 'any' ? 'any' : 'all',
        'poll_interval_seconds': _host.state.stepEditor.awaitPollInterval < 1 ? 1 : _host.state.stepEditor.awaitPollInterval,
      };
    }

    if (next['kind'] == 'debounce') {
      if (_host.state.stepEditor.debounceName.trim().isEmpty) {
        _setStepEditorError('Debounce needs a name');
        return false;
      }

      final debounceParams = <String, Object?>{
        ...parameters,
        'name': _host.state.stepEditor.debounceName.trim(),
        'delay_seconds': _host.state.stepEditor.debounceDelaySeconds < 1 ? 1 : _host.state.stepEditor.debounceDelaySeconds,
      };
      final triggerKey = _parseOptionalExpr('Debounce trigger key', _host.state.stepEditor.debounceTriggerKeyJson);

      if (!triggerKey.ok) {
        return false;
      }

      if (!triggerKey.present) {
        debounceParams.remove('trigger_key');
      } else {
        debounceParams['trigger_key'] = triggerKey.value;
      }

      next['parameters'] = debounceParams;
    }

    if (next['kind'] == 'collect') {
      if (_host.state.stepEditor.collectName.trim().isEmpty) {
        _setStepEditorError('Collect needs a name');
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'name': _host.state.stepEditor.collectName.trim(),
        'max': _host.state.stepEditor.collectMax < 1 ? 1 : _host.state.stepEditor.collectMax,
      };
    }

    if (next['kind'] == 'barrier') {
      if (_host.state.stepEditor.barrierName.trim().isEmpty) {
        _setStepEditorError('Barrier needs a name');
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'name': _host.state.stepEditor.barrierName.trim(),
        'count': _host.state.stepEditor.barrierCount < 1 ? 1 : _host.state.stepEditor.barrierCount,
        'poll_interval_seconds': _host.state.stepEditor.barrierPollInterval < 1 ? 1 : _host.state.stepEditor.barrierPollInterval,
      };
    }

    if (next['kind'] == 'circuit_breaker') {
      if (_host.state.stepEditor.circuitName.trim().isEmpty) {
        _setStepEditorError('Circuit breaker needs a name');
        return false;
      }

      next['parameters'] = {
        ...parameters,
        'name': _host.state.stepEditor.circuitName.trim(),
        'threshold': _host.state.stepEditor.circuitThreshold < 1 ? 1 : _host.state.stepEditor.circuitThreshold,
        'window_seconds': _host.state.stepEditor.circuitWindowSeconds < 1 ? 1 : _host.state.stepEditor.circuitWindowSeconds,
        'cooldown_seconds': _host.state.stepEditor.circuitCooldownSeconds < 0 ? 0 : _host.state.stepEditor.circuitCooldownSeconds,
      };
    }

    if (next['kind'] == 'event_source') {
      final eventParams = <String, Object?>{
        ...parameters,
        'event_type': _host.state.stepEditor.eventSourceType.trim().isNotEmpty ? _host.state.stepEditor.eventSourceType.trim() : '*',
      };
      final filter = _parseOptionalExpr('Event source filter', _host.state.stepEditor.eventSourceFilterJson);

      if (!filter.ok) {
        return false;
      }

      if (!filter.present) {
        eventParams.remove('filter');
      } else {
        eventParams['filter'] = filter.value;
      }

      final max = _host.state.stepEditor.eventSourceMax.truncate();

      if (max > 0) {
        eventParams['max'] = max;
      } else {
        eventParams.remove('max');
      }

      next['parameters'] = eventParams;
    }

    nodes[index] = next;

    if (_host.state.selectedStepId != next['id']) {
      _renameLayoutNode(_host.state.selectedStepId, next['id'] as String);
    }

    _host.state.selectedStepId = next['id'] as String;
    syncWorkflowDraftToJson();
    return true;
  }

  void populateStepEditor(String nodeId) {
    final match = ensureWorkflowNodes().where((item) => item['id'] == nodeId);

    if (match.isEmpty) {
      return;
    }

    final node = match.first;
    final parameters = asRecord(node['parameters']);
    final transitions = asRecord(node['transitions']);
    final wait = asRecord(node['wait']);
    final retry = asRecord(node['retry']);
    _host.internal.stepEditorHydrating = true;
    _host.internal.stepEditorApplyTimer?.cancel();
    _host.internal.stepEditorApplyTimer = null;

    final editor = _host.state.stepEditor;
    editor.id = nodeId;
    _host.state.selectedStepId = nodeId;
    editor.name = displayValue(node['name']);
    editor.kind = displayValue(node['kind']).isNotEmpty ? displayValue(node['kind']) : 'action';
    editor.approvalType = displayValue(parameters['approval_type']).isNotEmpty ? displayValue(parameters['approval_type']) : 'generic';
    editor.approvalPrompt = displayValue(parameters['prompt']).isNotEmpty ? displayValue(parameters['prompt']) : 'Approval required';
    editor.gateKind = displayValue(parameters['kind']).isNotEmpty ? displayValue(parameters['kind']) : 'manual';
    editor.gateWhenJson = pretty(parameters['when'] ?? <String, Object?>{});
    editor.gatePollInterval = _numOr(parameters['poll_interval'], 30);
    editor.gateTimeout = _numOr(parameters['timeout'], 0);
    editor.gateLabel = displayValue(parameters['label']);
    editor.signalName = displayValue(parameters['name']).isNotEmpty ? displayValue(parameters['name']) : 'signal';
    editor.conditionFallback = nodeRefId(transitions['next']) ?? '';
    editor.conditionBranches = asArray(transitions['branches']).map((branch) {
      final record = asRecord(branch);
      return ConditionBranchDraft(whenJson: pretty(record['when'] ?? <String, Object?>{}), target: nodeRefId(record['target']) ?? '');
    }).toList();
    editor.waitSeconds = _numOr(wait['seconds'], 60);
    editor.waitInitialStatus = displayValue(wait['initial_status']).isNotEmpty ? displayValue(wait['initial_status']) : 'waiting';
    editor.waitUntilStatus = displayValue(wait['until_status']);
    editor.waitJson = pretty(node['wait'] ?? <String, Object?>{});
    editor.loopItemsJson = pretty(parameters['items'] ?? <Object?>[]);
    editor.loopTarget = nodeRefId(parameters['target']) ?? '';
    editor.loopMaxIterations = _numOr(node['max_iterations'], 10);
    editor.switchValueJson = pretty(parameters['value'] ??
        valueRef('params', ['mode']));
    editor.switchCases = asArray(parameters['cases']).map((value) => switchCaseEditor(asRecord(value))).toList();
    editor.switchDefault = nodeRefId(parameters['default']) ?? '';
    editor.toggleValueJson = pretty(parameters['value'] ?? valueRef('config', ['flags', 'enabled']));
    editor.toggleOn = nodeRefId(parameters['on']) ?? '';
    editor.toggleOff = nodeRefId(parameters['off']) ?? '';
    editor.percentageKeyJson = pretty(parameters['key'] ?? valueRef('input', ['user_id']));
    editor.percentageBuckets = asArray(parameters['buckets']).map((bucket) {
      final record = asRecord(bucket);
      return PercentageBucketDraft(weight: _numOr(record['weight'], 0), target: nodeRefId(record['target']) ?? '');
    }).toList();
    editor.percentageDefault = nodeRefId(parameters['default']) ?? '';
    editor.parallelBranches = nodeRefArray(parameters['branches']);
    editor.joinWaitFor = nodeRefArray(parameters['wait_for']);
    editor.joinMode = branchPolicyName(parameters['mode'], 'all');
    editor.tryBody = nodeRefId(parameters['body']) ?? '';
    editor.tryCatch = nodeRefId(parameters['catch']) ?? '';
    editor.tryFinally = nodeRefId(parameters['finally']) ?? '';
    editor.mapItemsJson = pretty(parameters['items'] ?? <Object?>[]);
    editor.mapTarget = nodeRefId(parameters['target']) ?? '';
    editor.mapConcurrency = _numOr(parameters['concurrency'], 1);
    editor.raceBranches = nodeRefArray(parameters['branches']);
    editor.raceWinner = branchPolicyName(parameters['winner'], 'first_success');
    editor.outputEventType = displayValue(parameters['event_type']).isNotEmpty ? displayValue(parameters['event_type']) : 'workflow.output';
    editor.outputDataJson = _stepEditorJson(parameters['data']);
    editor.inputPrompt = displayValue(parameters['prompt']).isNotEmpty ? displayValue(parameters['prompt']) : 'Provide input';
    editor.configNameJson = _stepEditorJson(parameters['name'] ?? '');
    editor.configMetadataJson = _stepEditorJson(parameters['metadata'] ?? <String, Object?>{});
    editor.subflowId = displayValue(node['subflow_id']);
    editor.subflowParametersJson = pretty(node['parameters'] ?? <String, Object?>{});
    editor.assertAssertions = asArray(parameters['assertions']).map((assertion) {
      final record = asRecord(assertion);
      return AssertAssertionDraft(
        name: displayValue(record['name']),
        conditionJson: pretty(record['condition'] ?? true),
        message: displayValue(record['message']),
      );
    }).toList();
    editor.transformBindingsJson = pretty(parameters['bindings'] ?? <String, Object?>{});
    editor.auditActionJson = _stepEditorJson(parameters['action'] ?? 'workflow.audit');
    editor.auditActorJson = _optionalExprJson(parameters['actor']);
    editor.auditTargetJson = _optionalExprJson(parameters['target']);
    editor.auditReasonJson = _optionalExprJson(parameters['reason']);
    editor.checkpointName = displayValue(parameters['name']);
    editor.mutexName = displayValue(parameters['name']);
    editor.mutexPollInterval = _numOr(parameters['poll_interval_seconds'], 30);
    editor.throttleName = displayValue(parameters['name']);
    editor.throttleMaxPerWindow = _numOr(parameters['max_per_window'], 10);
    editor.throttleWindowSeconds = _numOr(parameters['window_seconds'], 60);
    editor.throttlePollInterval = _numOr(parameters['poll_interval_seconds'], 30);
    editor.awaitRunIdsJson = pretty(parameters['run_ids'] ?? valueRef('params', ['run_ids']));
    editor.awaitMode = parameters['mode'] == 'any' ? 'any' : 'all';
    editor.awaitPollInterval = _numOr(parameters['poll_interval_seconds'], 30);
    editor.debounceName = displayValue(parameters['name']);
    editor.debounceDelaySeconds = _numOr(parameters['delay_seconds'], 30);
    editor.debounceTriggerKeyJson = _optionalExprJson(parameters['trigger_key']);
    editor.collectName = displayValue(parameters['name']);
    editor.collectMax = _numOr(parameters['max'], 10);
    editor.barrierName = displayValue(parameters['name']);
    editor.barrierCount = _numOr(parameters['count'], 2);
    editor.barrierPollInterval = _numOr(parameters['poll_interval_seconds'], 30);
    editor.circuitName = displayValue(parameters['name']);
    editor.circuitThreshold = _numOr(parameters['threshold'], 5);
    editor.circuitWindowSeconds = _numOr(parameters['window_seconds'], 60);
    editor.circuitCooldownSeconds = _numOr(parameters['cooldown_seconds'], 60);
    editor.eventSourceType = displayValue(parameters['event_type']).isNotEmpty ? displayValue(parameters['event_type']) : '*';
    editor.eventSourceFilterJson = _optionalExprJson(parameters['filter']);
    editor.eventSourceMax = _numOr(parameters['max'], 0);
    editor.locked = isLockedWorkflowNode(node);
    editor.skipped = node['skipped'] == true;
    editor.maxAttempts = _numOr(retry['max_attempts'], 1);
    editor.timeoutSeconds = _numOr(node['timeout_seconds'], 0);
    final actionConfig = workflowNodeActionConfig(node);
    editor.actionName = actionConfig.provider;
    editor.actionFunction = actionConfig.action;
    // action nodes carry their inputs in action.configuration (merged with node.parameters); show the effective set.
    final actionInputs = node['kind'] == 'action' ? workflowNodeActionInputs(node) : asRecord(node['parameters']);
    editor.parametersJson = pretty(actionInputs);
    editor.transitionsJson = pretty(node['transitions'] ?? <String, Object?>{});
    _host.state.workflowInspectorMode = 'step';
    _runs.updateSelectedWorkflowNodeDetail();
    // mirrors the ts source's `setTimeout(() => {...}, 0)` deferred hydration-flag clear.
    Future.microtask(() => _host.internal.stepEditorHydrating = false);
  }

  List<WorkflowEdgeSemanticOption> workflowEdgeOptions(String sourceId) {
    final match = ensureWorkflowNodes().where((node) => node['id'] == sourceId);
    return match.isNotEmpty ? workflowEdgeSemanticOptions(match.first) : [];
  }

  WorkflowEdgeEditorDraft? openEdgeEditorDraft(String edgeId) {
    final match = _host.buildDraftGraphEdges().where((item) => item.id == edgeId);
    return match.isNotEmpty ? workflowEdgeEditorDraft(_host.state.workflowDraft, match.first) : null;
  }

  void selectGraphEdge(String edgeId) {
    _host.state.selectedStepId = '';
    _host.state.selectedGraphEdgeId = edgeId;
  }

  bool applyEdgeEditorDraft(WorkflowEdgeEditorDraft draft) {
    GraphEdgeModel? previousEdge;
    if (draft.edgeId.isNotEmpty) {
      final match = _host.buildDraftGraphEdges().where((edge) => edge.id == draft.edgeId);
      previousEdge = match.isNotEmpty ? match.first : null;
    }

    final result = applyWorkflowEdgeEditorDraft(_host.state.workflowDraft.definition, previousEdge, draft);

    if (!result.ok) {
      _host.setError(result.message!);
      return false;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    return true;
  }

  WorkflowEdgeEditorDraft? moveEdgeEditorItem(WorkflowEdgeEditorDraft draft, int direction) {
    final result = moveWorkflowEdgeEditorDraft(_host.state.workflowDraft.definition, draft, direction);

    if (!result.ok) {
      _host.setError(result.message!);
      return null;
    }

    syncWorkflowDraftToJson();
    populateStepEditor(draft.source);
    final movedMatch = _host.buildDraftGraphEdges().where(
          (edge) => edge.source == result.draft!.source && edge.target == result.draft!.target && workflowEdgeOptionId(edge) == result.draft!.optionId,
        );
    return movedMatch.isNotEmpty ? result.draft!.copyWith(edgeId: movedMatch.first.id) : result.draft;
  }

  bool moveSelectedEdge(int direction) {
    final draft = _host.state.selectedGraphEdgeId.isNotEmpty ? openEdgeEditorDraft(_host.state.selectedGraphEdgeId) : null;

    if (draft == null) {
      return false;
    }

    final moved = moveEdgeEditorItem(draft, direction);

    if (moved == null) {
      return false;
    }

    _host.state.selectedGraphEdgeId = moved.edgeId;
    return true;
  }

  bool reverseSelectedEdgeHandles() {
    final edge = _host.getSelectedGraphEdge();

    if (edge == null) {
      return false;
    }

    _dismissStepEditorForCanvasEdit();
    final data = edge.data;
    final semanticKey = data.transitionKey?.toJson() ?? (data.branchIndex != null ? 'branches.${data.branchIndex}' : parameterSemanticKey(data.parameterKey, data.parameterIndex));
    setWorkflowEdgeHandles(
      _host.state.workflowDraft.definition,
      edge.source,
      semanticKey,
      sourceHandle: edge.targetHandle,
      targetHandle: edge.sourceHandle,
      edgeStyle: data.edgeStyle,
    );
    syncWorkflowDraftToJson();
    _host.state.selectedGraphEdgeId = '';
    return true;
  }

  bool setEdgeLabelOffset(String edgeId, WorkflowEdgeLabelOffset? offset) {
    final match = _host.buildDraftGraphEdges().where((item) => item.id == edgeId);

    if (match.isEmpty) {
      return false;
    }

    _dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelOffset(_host.state.workflowDraft.definition, match.first, offset);
    syncWorkflowDraftToJson();
    return true;
  }

  bool setEdgeLabelAnchor(String edgeId, double? position) {
    final match = _host.buildDraftGraphEdges().where((item) => item.id == edgeId);

    if (match.isEmpty) {
      return false;
    }

    _dismissStepEditorForCanvasEdit();
    setWorkflowEdgeLabelAnchor(_host.state.workflowDraft.definition, match.first, position == null ? null : WorkflowEdgeLabelAnchor(position: position));
    syncWorkflowDraftToJson();
    return true;
  }

  void scheduleStepEditorApply() => applyStepEditor();

  bool applyGraphEdgeSemantic(GraphEdgeLike connection, String optionId, [String previousEdgeId = '']) {
    final source = connection.source;
    final target = connection.target;
    final sourceHandle = connection.sourceHandle;

    if (source.isEmpty || target.isEmpty) {
      return false;
    }

    _dismissStepEditorForCanvasEdit();

    if (isSameConnectionPointLoop(source: source, target: target, sourceHandle: sourceHandle, targetHandle: connection.targetHandle)) {
      _host.setError('Cannot connect a node handle back to itself');
      return false;
    }

    GraphEdgeModel? previousEdge;
    if (previousEdgeId.isNotEmpty) {
      final match = _host.buildDraftGraphEdges().where((edge) => edge.id == previousEdgeId);
      previousEdge = match.isNotEmpty ? match.first : null;
    }
    final previousDraft = previousEdge != null ? workflowEdgeEditorDraft(_host.state.workflowDraft, previousEdge) : null;
    final draft = (previousDraft ?? defaultEdgeEditorDraft()).copyWith(
      edgeId: previousEdgeId,
      source: source,
      target: target,
      optionId: optionId,
      sourceHandle: sourceHandle,
      targetHandle: connection.targetHandle,
    );
    return applyEdgeEditorDraft(draft);
  }

  void removeWorkflowEdgeById(String edgeId) {
    final match = _host.buildDraftGraphEdges().where((item) => item.id == edgeId);

    if (match.isEmpty) {
      return;
    }

    final edge = match.first;
    final sourceMatch = ensureWorkflowNodes().where((node) => node['id'] == edge.source);

    if (sourceMatch.isEmpty || !removeWorkflowEdge(sourceMatch.first, edge)) {
      return;
    }

    final data = edge.data;

    if (data.transitionKey != null) {
      removeWorkflowEdgeHandles(_host.state.workflowDraft.definition, edge.source, data.transitionKey!.toJson());
    }

    if (data.branchIndex != null) {
      removeWorkflowEdgeHandles(_host.state.workflowDraft.definition, edge.source, 'branches.${data.branchIndex}');
    }

    if (data.parameterKey != null) {
      removeWorkflowEdgeHandles(_host.state.workflowDraft.definition, edge.source, parameterSemanticKey(data.parameterKey, data.parameterIndex));
    }

    syncWorkflowDraftToJson();

    if (_host.state.selectedStepId.isNotEmpty) {
      populateStepEditor(_host.state.selectedStepId);
    }
  }

  void autoArrangeWorkflowNodes([WorkflowLayoutDirection? direction]) {
    if (!syncWorkflowJson()) {
      return;
    }

    final dir = direction ?? _host.state.workflowLayoutDirection;
    _host.state.workflowLayoutDirection = dir;
    final positions = autoArrangeWorkflowLayout(_host.state.workflowDraft.definition, dir);

    for (final entry in positions.entries) {
      _setGraphNodePosition(entry.key, GraphPosition(x: entry.value.x, y: entry.value.y));
    }

    autoArrangeWorkflowEdgeHandles(_host.state.workflowDraft.definition, positions);
    _host.state.workflowLayoutVersion += 1;
    syncWorkflowDraftToJson();
  }

  void scheduleWorkflowJsonSync() => syncWorkflowJson();

  void scheduleWorkflowWdlSync() {
    _host.internal.workflowWdlSyncTimer?.cancel();
    _host.internal.workflowWdlSyncTimer = Timer(const Duration(milliseconds: _workflowWdlSyncDelayMs), () {
      _host.internal.workflowWdlSyncTimer = null;
      syncWorkflowWdl();
    });
  }

  @override
  void scheduleWorkflowWdlRefresh() => refreshWorkflowWdl();

  @override
  void setWorkflowJsonSilently(String next) {
    _host.internal.workflowJsonWriteReleaseTimer?.cancel();
    _host.internal.workflowJsonWriteGuard = true;
    _host.state.workflowJson = next;
    _host.notify();
    _host.internal.workflowJsonWriteReleaseTimer = Timer(Duration.zero, () {
      _host.internal.workflowJsonWriteGuard = false;
      _host.internal.workflowJsonWriteReleaseTimer = null;
    });
  }

  @override
  void setWorkflowWdlSilently(String next) {
    _host.internal.workflowWdlWriteReleaseTimer?.cancel();
    _host.internal.workflowWdlWriteGuard = true;
    _host.state.workflowWdl = next;
    _host.notify();
    _host.internal.workflowWdlWriteReleaseTimer = Timer(Duration.zero, () {
      _host.internal.workflowWdlWriteGuard = false;
      _host.internal.workflowWdlWriteReleaseTimer = null;
    });
  }

  @override
  bool syncWorkflowJson() {
    final parsed = parseRequiredObject(_host.state.workflowJson);

    if (parsed == null) {
      _host.setError('Workflow definition must be a JSON object');
      return false;
    }

    final errors = validateWorkflowReferenceSyntax(parsed);

    if (errors.isNotEmpty) {
      _host.setError(errors.first);
      return false;
    }

    _host.state.workflowDraft.definition = parsed;
    _host.state.workflowDraft.definition['concurrency'] = _host.state.workflowConcurrency;
    _host.state.workflowDraft = normalizeWorkflowDefinition(_host.state.workflowDraft.cloneDeep());
    setWorkflowJsonSilently(pretty(_host.state.workflowDraft.definition));
    _host.state.isDirty = true;
    scheduleWorkflowWdlRefresh();
    return true;
  }

  void syncWorkflowDraftToJson() {
    // a graph edit is now the source of truth, so save should serialize the draft, not recompile wdl.
    _host.state.workflowEditorMode = WorkflowEditorMode.graph;
    _host.state.workflowDraft.definition['concurrency'] = _host.state.workflowConcurrency;
    _host.state.workflowDraft = normalizeWorkflowDefinition(_host.state.workflowDraft.cloneDeep());
    setWorkflowJsonSilently(pretty(_host.state.workflowDraft.definition));
    _host.state.isDirty = true;
    scheduleWorkflowWdlRefresh();
  }

  @override
  Future<bool> syncWorkflowWdl() async {
    _host.internal.workflowWdlSyncTimer = null;

    WorkflowDefinition compiled;
    final previousUi = isJsonObject(_host.state.workflowDraft.definition['ui']) ? asJsonObject(asJsonValue(_host.state.workflowDraft.definition['ui'])) : null;

    try {
      compiled = await api.compileWdl(_host.state.workflowWdl, _host.state.workflowDraft.enabled);
    } catch (err) {
      _host.setError('WDL compile error: ${errorMessage(err)}');
      return false;
    }

    _host.state.workflowDraft = WorkflowDefinition(
      id: _host.state.workflowDraft.id,
      name: compiled.name,
      version: compiled.version,
      enabled: _host.state.workflowDraft.enabled,
      inputType: compiled.inputType,
      definition: compiled.definition,
      orgId: _host.state.workflowDraft.orgId,
    );

    if (previousUi != null) {
      _host.state.workflowDraft.definition['ui'] = previousUi;
    }

    _host.state.workflowDraft.definition['concurrency'] = _host.state.workflowConcurrency;
    _host.state.workflowDraft = normalizeWorkflowDefinition(_host.state.workflowDraft.cloneDeep());
    setWorkflowJsonSilently(pretty(_host.state.workflowDraft.definition));
    _host.state.isDirty = true;
    return true;
  }

  @override
  Future<void> refreshWorkflowWdl() async {
    try {
      setWorkflowWdlSilently(await api.decompileToWdl(_host.state.workflowDraft.cloneDeep()));
      _host.state.workflowWdlError = '';
    } catch (err) {
      setWorkflowWdlSilently('');
      _host.state.workflowWdlError = errorMessage(err);
    }
    _host.notify();
  }

  List<JsonRecord> ensureWorkflowNodes() => _host.ensureWorkflowNodes();

  void _stripNewNodeConnections(JsonRecord node) {
    final transitions = asRecord(node['transitions']);
    final omitTransitionKeys = {...directTransitionKeys, 'branches'};
    node['transitions'] = {
      for (final entry in transitions.entries)
        if (!omitTransitionKeys.contains(entry.key)) entry.key: entry.value,
    };

    final parameters = asRecord(node['parameters']);
    final omitParameterKeys = {'target', 'default', 'body', 'catch', 'finally'};
    final cleanedParameters = <String, Object?>{
      for (final entry in parameters.entries)
        if (!omitParameterKeys.contains(entry.key)) entry.key: entry.value,
    };

    if (parameters['cases'] is List) {
      cleanedParameters['cases'] = <Object?>[];
    }

    if (parameters['branches'] is List) {
      cleanedParameters['branches'] = <Object?>[];
    }

    if (parameters['wait_for'] is List) {
      cleanedParameters['wait_for'] = <Object?>[];
    }

    node['parameters'] = cleanedParameters;
  }

  GraphPosition _graphCentroidPosition() {
    final positioned = _host
        .buildDraftGraphNodes()
        .map((node) => node.position)
        .where((p) => !p.x.isNaN && !p.y.isNaN)
        .toList();

    if (positioned.isEmpty) {
      return nextNodePosition(1);
    }

    var totalX = 0.0;
    var totalY = 0.0;
    for (final p in positioned) {
      totalX += p.x;
      totalY += p.y;
    }

    return GraphPosition(x: (totalX / positioned.length).roundToDouble(), y: (totalY / positioned.length).roundToDouble());
  }

  void setGraphNodePosition(String nodeId, GraphPosition position) => _setGraphNodePosition(nodeId, position);

  void dismissStepEditorForCanvasEdit() => _dismissStepEditorForCanvasEdit();

  void _setGraphNodePosition(String nodeId, GraphPosition position) {
    final definition = _host.state.workflowDraft.definition;
    final ui = asRecord(definition['ui']);
    definition['ui'] = ui;
    final layout = asRecord(ui['layout']);
    ui['layout'] = layout;
    final layoutNodes = asRecord(layout['nodes']);
    layout['nodes'] = layoutNodes;
    layoutNodes[nodeId] = {'x': position.x, 'y': position.y};
  }

  void _renameLayoutNode(String previousId, String nextId) {
    if (previousId.isEmpty || previousId == nextId) {
      return;
    }

    final layout = asRecord(asRecord(_host.state.workflowDraft.definition['ui'])['layout']);
    final layoutNodes = asRecord(layout['nodes']);

    if (layoutNodes[previousId] == null) {
      return;
    }

    final movedNode = layoutNodes[previousId];
    layout['nodes'] = {
      for (final entry in layoutNodes.entries)
        if (entry.key != previousId) entry.key: entry.value,
      nextId: movedNode,
    };
  }

  void addConditionBranchEditor() {
    _host.state.stepEditor.conditionBranches.add(ConditionBranchDraft(
      whenJson: pretty({
        'value': valueRef('params', ['value']),
        'equals': true,
      }),
      target: '',
    ));
    markWorkflowDirty();
  }

  void removeConditionBranchEditor(int index) {
    _host.state.stepEditor.conditionBranches.removeAt(index);
    final node = _host.getSelectedNode();

    if (node?['kind'] == 'condition') {
      removeConditionBranch(node!, index);
    }

    markWorkflowDirty();
  }

  void addSwitchCaseEditor() {
    _host.state.stepEditor.switchCases.add(SwitchCaseEditor(matchKind: 'equals', matchJson: pretty(true), target: ''));
    markWorkflowDirty();
  }

  void removeSwitchCaseEditor(int index) {
    _host.state.stepEditor.switchCases.removeAt(index);
    markWorkflowDirty();
  }

  void addAssertionEditor() {
    _host.state.stepEditor.assertAssertions.add(AssertAssertionDraft(
      name: '',
      conditionJson: pretty({
        'value': valueRef('params', ['value']),
        'equals': true,
      }),
      message: '',
    ));
    markWorkflowDirty();
  }

  void removeAssertionEditor(int index) {
    _host.state.stepEditor.assertAssertions.removeAt(index);
    markWorkflowDirty();
  }

  void addPercentageBucketEditor() {
    _host.state.stepEditor.percentageBuckets.add(PercentageBucketDraft(weight: 50, target: ''));
    markWorkflowDirty();
  }

  void removePercentageBucketEditor(int index) {
    _host.state.stepEditor.percentageBuckets.removeAt(index);
    markWorkflowDirty();
  }

  void addNodeRefEditor(List<String> list) {
    list.add('');
    markWorkflowDirty();
  }

  void removeNodeRefEditor(List<String> list, int index) {
    list.removeAt(index);
    markWorkflowDirty();
  }

  void markWorkflowDirty() {
    _host.state.isDirty = true;
  }

  void openStepEditor(String nodeId, {bool creating = false}) {
    _host.internal.stepEditorBaselineDefinition = creating ? null : Map<String, Object?>.from(jsonDecode(jsonEncode(_host.state.workflowDraft.definition)) as Map);
    populateStepEditor(nodeId);
    _host.state.stepEditorCreating = creating;
    _host.state.stepEditorCreatedNodeId = creating ? nodeId : '';
    _host.state.stepEditorError = '';
    _host.state.workflowInspectorMode = 'step';
    // the full modal supersedes the inline mini-editor.
    _host.state.inlineEditNodeId = '';
    _host.state.stepEditorOpen = true;
  }

  Future<void> submitStepEditor() async {
    if (!applyStepEditor()) {
      return;
    }

    _host.state.stepEditorOpen = false;
    _host.state.stepEditorCreating = false;
    _host.state.stepEditorCreatedNodeId = '';
    _host.state.selectedStepId = '';
    _host.state.inlineEditNodeId = '';
    // applying a step persists the workflow so canvas edits do not need a manual save.
    await _catalog.saveSelectedWorkflowBundle();
  }

  void _dismissStepEditorForCanvasEdit() {
    if (!_host.state.stepEditorOpen || _host.state.stepEditorCreating) {
      return;
    }

    _host.state.stepEditorOpen = false;
    _host.state.stepEditorError = '';
  }

  void closeStepEditor() {
    _host.internal.stepEditorApplyTimer?.cancel();
    _host.internal.stepEditorApplyTimer = null;

    if (_host.state.stepEditorCreating && _host.state.stepEditorCreatedNodeId.isNotEmpty) {
      final nodeId = _host.state.stepEditorCreatedNodeId;
      _host.state.workflowDraft.definition['nodes'] = ensureWorkflowNodes().where((node) => node['id'] != nodeId).toList();
      syncWorkflowDraftToJson();
    } else if (_host.internal.stepEditorBaselineDefinition != null) {
      _host.state.workflowDraft.definition = Map<String, Object?>.from(jsonDecode(jsonEncode(_host.internal.stepEditorBaselineDefinition)) as Map);
      syncWorkflowDraftToJson();
    }

    _host.state.selectedStepId = '';
    _host.state.inlineEditNodeId = '';
    _host.state.stepEditorOpen = false;
    _host.state.stepEditorCreating = false;
    _host.state.stepEditorCreatedNodeId = '';
    _host.state.stepEditorError = '';
    _host.internal.stepEditorBaselineDefinition = null;
    _host.internal.stepEditorHydrating = false;
  }

  void duplicateSelectedStep() {
    if (_host.state.selectedStepId.isEmpty || !_host.canRemoveSelectedStep()) {
      return;
    }

    final nodes = ensureWorkflowNodes();
    final match = nodes.where((node) => node['id'] == _host.state.selectedStepId);

    if (match.isEmpty) {
      return;
    }

    final source = match.first;
    final copy = Map<String, Object?>.from(jsonDecode(jsonEncode(source)) as Map);
    final copyId = uniqueWorkflowNodeId(nodes, '${source['id']}_copy');
    copy['id'] = copyId;
    _stripNewNodeConnections(copy);
    final position = _graphCentroidPosition();
    nodes.add(copy);
    _setGraphNodePosition(copyId, position);
    syncWorkflowDraftToJson();
    populateStepEditor(copyId);
    openStepEditor(copyId, creating: true);
  }

  void _setStepEditorError(String message) {
    _host.state.stepEditorError = message;
    _host.setError(message);
    _host.notify();
  }

  _StepJsonResult _parseStepJson(String label, String text) {
    final value = parseRequiredJson(text);

    if (value != null || text.trim() == 'null') {
      return _StepJsonResult.ok(value);
    }

    _setStepEditorError('$label must be valid JSON');
    return const _StepJsonResult.error();
  }

  String _stepEditorJson(Object? value) => const JsonEncoder.withIndent('  ').convert(value == null ? null : asJsonValue(value));

  String _optionalExprJson(Object? value) => value == null ? '' : pretty(asJsonValue(value));

  _OptionalExprResult _parseOptionalExpr(String label, String text) {
    if (text.trim().isEmpty) {
      return const _OptionalExprResult.absent();
    }

    final parsed = _parseStepJson(label, text);
    return parsed.ok ? _OptionalExprResult.ok(parsed.value) : const _OptionalExprResult.error();
  }

  bool isJsonObject(Object? value) => value is Map<String, Object?>;

  String _validateStepParameters(JsonRecord parameters) {
    if (_host.state.stepEditor.kind != 'action') {
      return '';
    }

    ProviderMetadata? provider;
    for (final item in _host.getProviders()) {
      if (item.name == _host.state.stepEditor.actionName) {
        provider = item;
        break;
      }
    }

    ActionMetadata? action;
    if (provider != null) {
      for (final item in provider.actions) {
        if (item.functionName == _host.state.stepEditor.actionFunction) {
          action = item;
          break;
        }
      }
    }

    if (action == null) {
      return 'Select a valid task provider action';
    }

    for (final parameter in action.parameters) {
      if (!parameter.required) {
        continue;
      }

      final value = parameters[parameter.name];

      if (isBlankValue(value)) {
        return '${parameter.label ?? parameter.name} is required';
      }

      final typeError = validateJsonValueType(value, parameter.ty, parameter.label ?? parameter.name);

      if (typeError.isNotEmpty) {
        return typeError;
      }
    }

    return '';
  }

  num _numOr(Object? value, num fallback) => value is num ? value : fallback;
}
