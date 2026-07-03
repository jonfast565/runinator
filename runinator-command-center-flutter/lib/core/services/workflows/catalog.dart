// port of core/services/workflows/catalog.ts.

import '../../api/command_center_api.dart' as api;
import '../../domain/json.dart';
import '../../domain/models/index.dart';
import '../../utils/format.dart' show pretty, errorMessage;
import '../../utils/json_utils.dart' show cloneJson, parseRequiredObject;
import '../../utils/zip.dart';
import '../../workflow/editor_defaults.dart';
import '../../workflow/workflow_helpers.dart';
import 'host.dart';
import 'state.dart';

/// avoids a circular import between catalog/editor exactly like the ts source's
/// WorkflowEditorPeer interface.
abstract class WorkflowEditorPeer {
  void setWorkflowJsonSilently(String next);

  void setWorkflowWdlSilently(String next);

  Future<void> refreshWorkflowWdl();

  bool syncWorkflowJson();

  Future<bool> syncWorkflowWdl();

  void scheduleWorkflowWdlRefresh();
}

abstract class WorkflowRunsPeer {
  void clearWorkflowRunGates();
}

class WorkflowCatalogService {
  WorkflowCatalogService(this._host, this._editor, this._runs);

  final WorkflowServiceHost _host;
  final WorkflowEditorPeer _editor;
  final WorkflowRunsPeer _runs;

  Future<void> refreshWorkflows() async {
    List<WorkflowDefinition> workflows;
    try {
      workflows = await _host.runOperation('Refreshing workflows', api.fetchWorkflows);
    } catch (_) {
      workflows = [];
    }
    _host.state.workflows = workflows;

    if (_host.state.selectedWorkflowId == null && _host.state.workflows.isNotEmpty) {
      _host.state.selectedWorkflowId = _host.state.workflows.first.id;
    }

    WorkflowDefinition? workflow;
    for (final item in _host.state.workflows) {
      if (item.id == _host.state.selectedWorkflowId) {
        workflow = item;
        break;
      }
    }
    workflow ??= _host.state.workflows.isNotEmpty ? _host.state.workflows.first : null;

    if (workflow != null && !_host.state.isDirty) {
      await selectWorkflow(workflow);
    }
    _host.notify();
  }

  void clearServiceState({bool discardDraft = false}) {
    _host.state.workflows = [];
    _host.state.workflowRuns = [];
    _host.state.workflowRunDetail = null;
    _host.state.openRunIds = [];
    _host.internal.runDetailById.clear();
    _host.internal.pendingBreakpointPatch = null;
    _host.state.workflowNodeDetailExtra = '';
    _host.state.selectedWorkflowRunId = null;
    _host.state.selectedWorkflowRunNodeId = '';
    _host.state.selectedWorkflowNodeRunId = null;
    _runs.clearWorkflowRunGates();
    clearWorkflowTriggerState();

    if (_host.state.isDirty && !discardDraft) {
      return;
    }

    _host.state.isDirty = false;
    _host.state.selectedWorkflowId = null;
    _host.state.workflowDraft = newWorkflowDraft();
    _editor.setWorkflowJsonSilently(pretty(_host.state.workflowDraft.definition));
    _editor.setWorkflowWdlSilently('');
    _host.state.workflowWdlError = '';
    _host.state.selectedStepId = '';
    _host.state.stepEditorOpen = false;
    _host.notify();
  }

  Future<void> selectWorkflow(WorkflowDefinition workflow) async {
    final isSwitch = _host.state.selectedWorkflowId != workflow.id;
    _host.state.selectedWorkflowId = workflow.id;
    _host.state.workflowDraft = normalizeWorkflowDefinition(workflow.cloneDeep());
    _host.state.workflowConcurrency = (_host.state.workflowDraft.definition['concurrency'] as num?) ?? 1;
    _editor.setWorkflowJsonSilently(pretty(_host.state.workflowDraft.definition));

    if (isSwitch) {
      _host.state.selectedStepId = '';
      clearWorkflowTriggerState();
      _host.state.stepEditorOpen = false;
    }

    _host.state.workflowEditorMode = WorkflowEditorMode.graph;
    _host.state.isDirty = false;
    // the graph derives from the draft; the wdl pane is decompiled, so refresh it for the newly
    // selected workflow since both panes are visible at once.
    await _editor.refreshWorkflowWdl();
    _host.notify();
  }

  void addWorkflow() {
    final workflow = newWorkflowDraft();
    _host.state.workflows.add(workflow);
    // fire-and-forget, mirrors the ts source's `void selectWorkflow(workflow)`.
    selectWorkflow(workflow);
    _host.notify();
  }

  String workflowNameForRun(RunSummary run) {
    for (final workflow in _host.state.workflows) {
      if (workflow.id == run.workflowId) return workflow.name;
    }
    return '';
  }

  Future<void> exportWorkflowWdl() async {
    try {
      final source = await api.decompileToWdl(_host.state.workflowDraft.cloneDeep());
      final name = _host.state.workflowDraft.name.trim().isNotEmpty ? _host.state.workflowDraft.name.trim() : 'workflow';
      final fileName = '${name.replaceAll(RegExp(r'[^a-z0-9._-]+', caseSensitive: false), '_')}.wdl';
      _host.downloadTextFile(fileName, source, 'text/plain');
      _host.setStatus('Exported $fileName');
    } catch (err) {
      _host.setError('Could not export this workflow as WDL (${errorMessage(err)}).');
    }
    _host.notify();
  }

  Future<void> exportWorkflowPack() async {
    final allWorkflows = _host.state.workflows.where((w) => w.id != null).toList();

    if (allWorkflows.isEmpty) {
      _host.setError('No workflows to export.');
      return;
    }

    await _host.runOperation('Exporting workflow pack', () async {
      final entries = <ZipEntry>[];
      final manifestWorkflows = <String>[];
      final triggers = <WorkflowTrigger>[];
      final usedNames = <String>{};
      final skipped = <String>[];

      for (final workflow in allWorkflows) {
        String source;

        try {
          source = await api.decompileToWdl(workflow.cloneDeep());
        } catch (_) {
          skipped.add(workflow.name.isNotEmpty ? workflow.name : 'workflow ${workflow.id}');
          continue;
        }

        var slug = (workflow.name.trim().isNotEmpty ? workflow.name.trim() : 'workflow-${workflow.id}')
            .replaceAll(RegExp(r'[^a-z0-9._-]+', caseSensitive: false), '_');

        while (usedNames.contains(slug)) {
          slug = '${slug}_${workflow.id}';
        }

        usedNames.add(slug);
        final fileName = '$slug.wdl';
        entries.add(ZipEntry(name: fileName, content: source));
        manifestWorkflows.add(fileName);
        try {
          triggers.addAll(await api.fetchWorkflowTriggers(workflow.id!));
        } catch (_) {
          // best effort, mirrors the ts source's .catch(() => []).
        }
      }

      if (entries.isEmpty) {
        throw StateError('no workflows could be decompiled to WDL');
      }

      final manifest = {
        'version': 1,
        'workflows': manifestWorkflows,
        'triggers': triggers.map((t) => t.toJson()).toList(),
      };
      entries.insert(0, ZipEntry(name: 'pack.wdlp', content: pretty(manifest)));
      _host.downloadBlob('runinator-pack.zip', createZip(entries));
      final note = skipped.isNotEmpty ? ' (skipped ${skipped.length} non-WDL: ${skipped.join(", ")})' : '';
      _host.setStatus('Exported ${entries.length - 1} workflow(s) to runinator-pack.zip$note');
    });
    _host.notify();
  }

  void moveWorkflowSelection(int delta) {
    final list = _host.getFilteredWorkflows();

    if (list.isEmpty) {
      return;
    }

    final current = list.indexWhere((workflow) => workflow.id == _host.state.selectedWorkflowId);
    selectWorkflow(list[boundedIndex(current, delta, list.length)]);
    _host.notify();
  }

  void openWorkflowSettings() {
    _host.state.workflowSettingsOpen = true;
    refreshWorkflowTriggers();
    _host.notify();
  }

  void closeWorkflowSettings() {
    _host.state.workflowSettingsOpen = false;
    closeTriggerEditor();
    _host.notify();
  }

  Future<void> refreshWorkflowTriggers() async {
    final workflowId = _host.state.workflowDraft.id;

    if (workflowId == null) {
      _host.state.workflowTriggers = [];
      closeTriggerEditor();
      return;
    }

    try {
      _host.state.workflowTriggers = await _host.runOperation('Loading workflow triggers', () => api.fetchWorkflowTriggers(workflowId));
    } catch (_) {
      _host.state.workflowTriggers = [];
    }
    _host.notify();
  }

  void clearWorkflowTriggerState() {
    _host.state.workflowTriggers = [];
    closeTriggerEditor();
    _host.notify();
  }

  void addWorkflowTrigger([WorkflowTriggerKind kind = WorkflowTriggerKind.cron]) {
    if (_host.state.workflowDraft.id == null) {
      return;
    }

    _host.state.triggerDraft = newWorkflowTriggerDraft(_host.state.workflowDraft.id!, kind);
    _host.state.triggerJson = TriggerJsonDraft(
      configuration: pretty(_host.state.triggerDraft.configuration),
      metadata: pretty(_host.state.triggerDraft.metadata),
    );
    _host.state.triggerEditorCreating = true;
    _host.state.triggerEditorError = '';
    _host.state.triggerEditorOpen = true;
    _host.notify();
  }

  void editWorkflowTrigger(WorkflowTrigger trigger) {
    final clone = trigger.cloneDeep();
    _host.state.triggerDraft = WorkflowTrigger(
      id: clone.id,
      workflowId: clone.workflowId,
      kind: clone.kind,
      enabled: clone.enabled,
      configuration: clone.configuration,
      nextExecution: _triggerDateForInput(trigger.nextExecution),
      blackoutStart: _triggerDateForInput(trigger.blackoutStart),
      blackoutEnd: _triggerDateForInput(trigger.blackoutEnd),
      metadata: clone.metadata,
      createdAt: clone.createdAt,
      updatedAt: clone.updatedAt,
    );
    _host.state.triggerJson = TriggerJsonDraft(configuration: pretty(trigger.configuration), metadata: pretty(trigger.metadata));
    _host.state.triggerEditorCreating = false;
    _host.state.triggerEditorError = '';
    _host.state.triggerEditorOpen = true;
    _host.notify();
  }

  void closeTriggerEditor() {
    _host.state.triggerEditorOpen = false;
    _host.state.triggerEditorCreating = false;
    _host.state.triggerEditorError = '';
    _host.notify();
  }

  void setTriggerKind(WorkflowTriggerKind kind) {
    final configuration = _host.state.triggerEditorCreating ? defaultTriggerConfiguration(kind) : _host.state.triggerDraft.configuration;
    _host.state.triggerDraft = WorkflowTrigger(
      id: _host.state.triggerDraft.id,
      workflowId: _host.state.triggerDraft.workflowId,
      kind: kind,
      enabled: _host.state.triggerDraft.enabled,
      configuration: configuration,
      nextExecution: _host.state.triggerDraft.nextExecution,
      blackoutStart: _host.state.triggerDraft.blackoutStart,
      blackoutEnd: _host.state.triggerDraft.blackoutEnd,
      metadata: _host.state.triggerDraft.metadata,
      createdAt: _host.state.triggerDraft.createdAt,
      updatedAt: _host.state.triggerDraft.updatedAt,
    );

    if (_host.state.triggerEditorCreating) {
      _host.state.triggerJson = TriggerJsonDraft(configuration: pretty(configuration), metadata: _host.state.triggerJson.metadata);
    }
    _host.notify();
  }

  Future<void> submitWorkflowTrigger() async {
    _host.state.triggerEditorError = '';

    if (_host.state.workflowDraft.id == null) {
      return;
    }

    final configuration = parseRequiredObject(_host.state.triggerJson.configuration);
    final metadata = parseRequiredObject(_host.state.triggerJson.metadata);

    if (configuration == null || metadata == null) {
      _host.state.triggerEditorError = configuration != null ? 'Trigger metadata must be a JSON object' : 'Trigger configuration must be a JSON object';
      _host.setError(_host.state.triggerEditorError);
      return;
    }

    final draft = _host.state.triggerDraft;
    final trigger = WorkflowTrigger(
      id: draft.id,
      workflowId: _host.state.workflowDraft.id!,
      kind: draft.kind,
      enabled: draft.enabled,
      configuration: configuration,
      metadata: metadata,
      nextExecution: dateTimeLocalToIso(draft.nextExecution),
      blackoutStart: dateTimeLocalToIso(draft.blackoutStart),
      blackoutEnd: dateTimeLocalToIso(draft.blackoutEnd),
      createdAt: draft.createdAt,
      updatedAt: draft.updatedAt,
    );
    final saved = await _host.runOperation('Saving workflow trigger', () => api.saveWorkflowTrigger(trigger, _host.state.triggerEditorCreating));
    _host.setStatus('Workflow trigger saved: ${saved.kind.wire}');
    closeTriggerEditor();
    await refreshWorkflowTriggers();
    _host.notify();
  }

  Future<void> deleteSelectedWorkflowTrigger(WorkflowTrigger trigger) async {
    final triggerId = trigger.id;

    if (triggerId == null) {
      return;
    }

    if (!_host.confirm('Delete ${trigger.kind.wire} trigger $triggerId?')) {
      return;
    }

    final response = await _host.runOperation('Deleting workflow trigger', () => api.deleteWorkflowTrigger(triggerId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to delete workflow trigger');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Workflow trigger deleted');

    if (_host.state.triggerDraft.id == trigger.id) {
      closeTriggerEditor();
    }

    await refreshWorkflowTriggers();
  }

  String triggerCronSummary(WorkflowTrigger trigger) {
    final cron = trigger.configuration['cron'];
    return (cron is String && cron.trim().isNotEmpty) ? cron : '';
  }

  String triggerDateForInput(String? value) => _triggerDateForInput(value);

  String _triggerDateForInput(String? value) {
    if (value == null || value.isEmpty) {
      return '';
    }

    final date = DateTime.tryParse(value);

    if (date == null) {
      return '';
    }

    return date.toLocal().toIso8601String().substring(0, 16);
  }

  List<WorkflowTrigger> workflowSaveTriggers(String? workflowId) {
    if (workflowId == null) {
      return [];
    }

    return _host.state.workflowTriggers.where((t) => t.workflowId == workflowId).map((t) => t.cloneDeep()).toList();
  }

  Future<api.WorkflowWdlSaveRequest> workflowWdlSaveRequest() async {
    final workflow = _host.state.workflowDraft.cloneDeep();
    final workflowId = workflow.id;
    final source = await api.decompileToWdl(workflow);
    final triggers = workflowId == null ? <WorkflowTrigger>[] : workflowSaveTriggers(workflowId);
    JsonRecord? ui;

    if (isJsonObject(workflow.definition['ui'])) {
      ui = cloneJson(workflow.definition['ui'] as JsonRecord);
    }

    return api.WorkflowWdlSaveRequest(source: source, enabled: workflow.enabled, workflowId: workflowId, triggers: triggers, ui: ui);
  }

  Future<void> saveSelectedWorkflowBundle() async {
    final synced = _host.state.workflowEditorMode == WorkflowEditorMode.wdl ? await _editor.syncWorkflowWdl() : _editor.syncWorkflowJson();

    if (!synced) {
      return;
    }

    _host.state.workflowDraft.definition['concurrency'] = _host.state.workflowConcurrency;
    _host.state.workflowDraft = normalizeWorkflowDefinition(_host.state.workflowDraft.cloneDeep());
    final saved = await _host.runOperation('Saving workflow', () async => api.saveWorkflowWdl(await workflowWdlSaveRequest()));
    final savedWorkflow = saved.workflows.isNotEmpty ? saved.workflows.first : null;

    if (savedWorkflow == null) {
      _host.setError('Workflow bundle save returned no workflow');
      return;
    }

    _host.state.workflowDraft = normalizeWorkflowDefinition(savedWorkflow.cloneDeep());
    _host.state.workflowTriggers = saved.triggers.where((t) => t.workflowId == _host.state.workflowDraft.id).toList();
    _editor.setWorkflowJsonSilently(pretty(_host.state.workflowDraft.definition));
    _editor.scheduleWorkflowWdlRefresh();
    _host.setStatus('Workflow saved: ${savedWorkflow.name}');
    _host.state.isDirty = false;
    _host.state.selectedWorkflowId = savedWorkflow.id;
    await refreshWorkflows();
    _host.notify();
  }

  Future<void> deleteSelectedWorkflow() async {
    final workflow = _host.getSelectedWorkflow();

    if (workflow?.id == null) {
      return;
    }

    if (!_host.confirm(
      'Delete workflow "${workflow!.name}"?\n\nThis permanently deletes the workflow along with ALL of its runs and their execution history. This cannot be undone.',
    )) {
      return;
    }

    final workflowId = workflow.id!;
    final response = await _host.runOperation('Deleting workflow ${workflow.name}', () => api.deleteWorkflow(workflowId));

    if (!response.success) {
      _host.setError(response.message.isNotEmpty ? response.message : 'Failed to delete workflow');
      return;
    }

    _host.setStatus(response.message.isNotEmpty ? response.message : 'Workflow deleted: ${workflow.name}');
    closeWorkflowSettings();
    _host.state.workflows = _host.state.workflows.where((item) => item.id != workflowId).toList();
    _host.state.selectedWorkflowId = _host.state.workflows.isNotEmpty ? _host.state.workflows.first.id : null;

    if (_host.state.workflows.isNotEmpty) {
      await selectWorkflow(_host.state.workflows.first);
    } else {
      _host.state.workflowDraft = newWorkflowDraft();
      _editor.setWorkflowJsonSilently(pretty(_host.state.workflowDraft.definition));
      _editor.setWorkflowWdlSilently('');
      _host.state.workflowWdlError = '';
      _host.state.workflowRuns = [];
      _host.state.workflowRunDetail = null;
      _host.state.selectedWorkflowRunId = null;
      _host.state.isDirty = false;
    }
    _host.notify();
  }

  Future<void> duplicateSelectedWorkflow([String bump = 'minor']) async {
    final workflow = _host.getSelectedWorkflow();

    if (workflow?.id == null) {
      return;
    }

    if (_host.state.isDirty) {
      _host.setError('Save or discard the current changes before duplicating this workflow.');
      return;
    }

    final workflowId = workflow!.id!;
    WorkflowDefinition? copy;
    try {
      copy = await _host.runOperation('Duplicating workflow ${workflow.name}', () => api.duplicateWorkflow(workflowId, bump));
    } catch (error) {
      _host.setError(error.toString());
      copy = null;
    }

    if (copy == null) {
      return;
    }

    await refreshWorkflows();
    _host.state.selectedWorkflowId = copy.id;
    await selectWorkflow(copy);
    _host.setStatus('Duplicated ${workflow.name} as v${copy.version}');
    _host.notify();
  }
}
