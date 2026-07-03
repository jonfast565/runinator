import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/app_service.dart';
import '../../core/services/workflow_run_extras_service.dart';
import '../../core/services/workflows/state.dart';
import '../../core/services/workflows_service.dart';
import '../../core/utils/format.dart';
import '../../core/workflow/workflow_helpers.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';
import '../workflow/debug_control_bar.dart';
import '../workflow/log_panel.dart';
import '../workflow/run_tabs_bar.dart';
import '../workflow/watch_expressions_panel.dart';
import '../workflow/workflow_graph_canvas.dart';

class RunsView extends ConsumerStatefulWidget {
  const RunsView({super.key});

  @override
  ConsumerState<RunsView> createState() => _RunsViewState();
}

class _RunsViewState extends ConsumerState<RunsView> {
  List<RunChunk> _nodeChunks = const [];
  List<RunArtifact> _nodeArtifacts = const [];
  List<WorkflowRunArtifact> _runArtifacts = const [];
  var _loadingLogs = false;
  var _loadingArtifacts = false;

  WorkflowNodeRun? _selectedNode(WorkflowServicesState workflows) {
    final nodeId = workflows.selectedWorkflowRunNodeId;
    for (final node in workflows.workflowRunDetail?.nodes ?? const <WorkflowNodeRun>[]) {
      if (node.nodeId == nodeId) return node;
    }
    return null;
  }

  Future<void> _loadNodeData(String? nodeRunId) async {
    if (nodeRunId == null || nodeRunId.isEmpty) {
      setState(() {
        _nodeChunks = const [];
        _nodeArtifacts = const [];
      });
      return;
    }

    setState(() => _loadingLogs = true);
    try {
      final extras = ref.read(workflowRunExtrasServiceProvider);
      final results = await Future.wait([
        extras.fetchNodeRunChunks(nodeRunId),
        extras.fetchNodeRunArtifacts(nodeRunId),
      ]);
      if (mounted) {
        setState(() {
          _nodeChunks = results[0] as List<RunChunk>;
          _nodeArtifacts = results[1] as List<RunArtifact>;
        });
      }
    } finally {
      if (mounted) setState(() => _loadingLogs = false);
    }
  }

  Future<void> _loadRunArtifacts(String? runId) async {
    if (runId == null || runId.isEmpty) {
      setState(() => _runArtifacts = const []);
      return;
    }
    setState(() => _loadingArtifacts = true);
    try {
      final artifacts = await ref.read(workflowRunExtrasServiceProvider).fetchRunArtifacts(runId);
      if (mounted) setState(() => _runArtifacts = artifacts);
    } finally {
      if (mounted) setState(() => _loadingArtifacts = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final workflows = ref.watch(workflowsProvider);
    final notifier = ref.read(workflowsProvider.notifier);
    final query = ref.read(appProvider.notifier).normalizedSearch;
    final runs = workflows.workflowRuns;
    final filtered = query.isEmpty
        ? runs
        : runs.where((run) => [run.id, run.name, run.workflowId, run.status].any((v) => v?.toLowerCase().contains(query) ?? false)).toList();

    final detail = workflows.workflowRunDetail;
    final workflow = notifier.host.getWorkflowRunWorkflow() ?? workflows.workflowDraft;
    final runNodes = buildGraphNodeModels(
      workflow,
      detail,
      subflowNames: notifier.host.getSubflowNames(),
      providers: notifier.host.getProviders(),
    );

    final nodeRunId = workflows.selectedWorkflowNodeRunId;
    ref.listen(workflowsProvider.select((s) => s.selectedWorkflowNodeRunId), (prev, next) {
      if (prev != next) _loadNodeData(next);
    });
    ref.listen(workflowsProvider.select((s) => s.selectedWorkflowRunId), (prev, next) {
      if (prev != next) _loadRunArtifacts(next);
    });

    if (nodeRunId != null && nodeRunId.isNotEmpty && _nodeChunks.isEmpty && !_loadingLogs) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _loadNodeData(nodeRunId));
    }
    if (workflows.selectedWorkflowRunId != null && _runArtifacts.isEmpty && !_loadingArtifacts) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _loadRunArtifacts(workflows.selectedWorkflowRunId));
    }

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        initialFirstFraction: 0.28,
        minFirst: 260,
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: 'Recent Runs',
                actions: [
                  CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.runs.fetchRecentWorkflowRuns()),
                ],
              ),
              Expanded(
                child: filtered.isEmpty
                    ? EmptyState(message: query.isEmpty ? 'No runs yet.' : 'No runs match "$query".')
                    : ListView.builder(
                        itemCount: filtered.length,
                        itemBuilder: (context, index) {
                          final run = filtered[index];
                          final selected = run.id == workflows.selectedWorkflowRunId;
                          return ListTile(
                            selected: selected,
                            title: Text(run.name ?? run.id ?? 'Run', style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600)),
                            subtitle: Text('${run.status ?? 'unknown'} · ${run.workflowId ?? ''}', style: const TextStyle(fontSize: 11)),
                            trailing: StatusBadge(run.status),
                            onTap: () => notifier.runs.selectWorkflowRun(run),
                          );
                        },
                      ),
              ),
            ],
          ),
        ),
        second: detail == null
            ? const EmptyState(message: 'Select a run to inspect.')
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  const RunTabsBar(),
                  Expanded(
                    child: SplitPane(
                      initialFirstFraction: 0.55,
                      first: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          PanelToolbar(
                            title: detail.run.name ?? detail.run.id ?? 'Run',
                            actions: [
                              if (notifier.host.canCancelWorkflowRun())
                                CcButton(icon: IconName.stop, label: 'Cancel', variant: CcButtonVariant.danger, dense: true, onPressed: () => notifier.runs.cancelSelectedWorkflowRun()),
                            ],
                          ),
                          if (notifier.host.isDebugRun()) ...[
                            const Padding(padding: EdgeInsets.fromLTRB(12, 0, 12, 8), child: DebugControlBar()),
                          ],
                          Expanded(
                            child: Padding(
                              padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
                              child: WorkflowGraphCanvas(
                                nodes: runNodes,
                                edges: buildGraphEdgeModels(workflow),
                                selectedNodeId: workflows.selectedWorkflowRunNodeId.isEmpty ? null : workflows.selectedWorkflowRunNodeId,
                                readOnly: true,
                                onNodeClick: (nodeId) {
                                  notifier.runs.selectWorkflowRunNode(nodeId);
                                  notifier.runs.updateSelectedWorkflowNodeDetail();
                                },
                              ),
                            ),
                          ),
                        ],
                      ),
                      second: PanelCard(
                        child: DefaultTabController(
                          length: 4,
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              const TabBar(
                                isScrollable: true,
                                tabs: [
                                  Tab(text: 'Output'),
                                  Tab(text: 'Logs'),
                                  Tab(text: 'Watch'),
                                  Tab(text: 'Artifacts'),
                                ],
                              ),
                              Expanded(
                                child: TabBarView(
                                  children: [
                                    SingleChildScrollView(
                                      padding: const EdgeInsets.all(12),
                                      child: Column(
                                        crossAxisAlignment: CrossAxisAlignment.start,
                                        children: [
                                          Text('Status: ${detail.run.status ?? 'unknown'}', style: const TextStyle(fontWeight: FontWeight.w700)),
                                          if (detail.run.startedAt != null)
                                            Text('Started: ${detail.run.startedAt}', style: const TextStyle(fontSize: 12, color: AppColors.textMuted)),
                                          const SizedBox(height: 12),
                                          SizedBox(
                                            height: 240,
                                            child: JsonEditor(value: pretty(detail.run.outputJson), onChanged: (_) {}, readOnly: true),
                                          ),
                                          if (_selectedNode(workflows) != null) ...[
                                            const SizedBox(height: 16),
                                            Text('Node: ${_selectedNode(workflows)!.nodeId}', style: const TextStyle(fontWeight: FontWeight.w700)),
                                            SizedBox(
                                              height: 160,
                                              child: JsonEditor(value: pretty(_selectedNode(workflows)!.outputJson), onChanged: (_) {}, readOnly: true),
                                            ),
                                          ],
                                        ],
                                      ),
                                    ),
                                    _loadingLogs
                                        ? const Center(child: CircularProgressIndicator())
                                        : LogPanel(chunks: _nodeChunks),
                                    const SingleChildScrollView(padding: EdgeInsets.all(12), child: WatchExpressionsPanel()),
                                    _ArtifactsPanel(
                                      loading: _loadingArtifacts,
                                      nodeArtifacts: _nodeArtifacts,
                                      runArtifacts: _runArtifacts,
                                      onDownload: (id, name) => ref.read(workflowRunExtrasServiceProvider).downloadArtifact(id, name),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                ],
              ),
      ),
    );
  }
}

class _ArtifactsPanel extends StatelessWidget {
  const _ArtifactsPanel({
    required this.loading,
    required this.nodeArtifacts,
    required this.runArtifacts,
    required this.onDownload,
  });

  final bool loading;
  final List<RunArtifact> nodeArtifacts;
  final List<WorkflowRunArtifact> runArtifacts;
  final Future<void> Function(String id, String name) onDownload;

  @override
  Widget build(BuildContext context) {
    if (loading) return const Center(child: CircularProgressIndicator());
    return ListView(
      padding: const EdgeInsets.all(12),
      children: [
        const Text('Node artifacts', style: TextStyle(fontWeight: FontWeight.w700)),
        if (nodeArtifacts.isEmpty) const Text('No node artifacts.', style: TextStyle(fontSize: 12, color: AppColors.textMuted)),
        for (final item in nodeArtifacts)
          ListTile(
            dense: true,
            title: Text(item.name),
            subtitle: Text(item.mimeType),
            trailing: IconButton(icon: const Icon(Icons.download, size: 16), onPressed: () => onDownload(item.id, item.name)),
          ),
        const SizedBox(height: 16),
        const Text('Run artifacts', style: TextStyle(fontWeight: FontWeight.w700)),
        if (runArtifacts.isEmpty) const Text('No run artifacts.', style: TextStyle(fontSize: 12, color: AppColors.textMuted)),
        for (final item in runArtifacts)
          ListTile(
            dense: true,
            title: Text(item.name),
            subtitle: Text(item.nodeId),
            trailing: IconButton(icon: const Icon(Icons.download, size: 16), onPressed: () => onDownload(item.artifactId, item.name)),
          ),
      ],
    );
  }
}
