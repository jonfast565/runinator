import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/app_service.dart';
import '../../core/services/workflow_run_extras_service.dart';
import '../../core/services/workflows_service.dart';
import '../../core/utils/format.dart';
import '../../core/workflow/workflow_helpers.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';
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
  var _loadingLogs = false;

  Future<void> _loadNodeLogs(String? nodeRunId) async {
    if (nodeRunId == null || nodeRunId.isEmpty) {
      setState(() => _nodeChunks = const []);
      return;
    }

    setState(() => _loadingLogs = true);
    try {
      final chunks = await ref.read(workflowRunExtrasServiceProvider).fetchNodeRunChunks(nodeRunId);
      if (mounted) setState(() => _nodeChunks = chunks);
    } finally {
      if (mounted) setState(() => _loadingLogs = false);
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
      if (prev != next) {
        _loadNodeLogs(next);
      }
    });

    if (nodeRunId != null && nodeRunId.isNotEmpty && _nodeChunks.isEmpty && !_loadingLogs) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _loadNodeLogs(nodeRunId));
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
                              if (notifier.host.canStepWorkflowRun())
                                CcButton(icon: IconName.step, label: 'Step', dense: true, onPressed: () => notifier.runs.stepSelectedWorkflowRun()),
                              if (notifier.host.canContinueWorkflowRun())
                                CcButton(icon: IconName.continue_, label: 'Continue', variant: CcButtonVariant.primary, dense: true, onPressed: () => notifier.runs.continueSelectedWorkflowRun()),
                            ],
                          ),
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
                          length: 3,
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              const TabBar(
                                tabs: [
                                  Tab(text: 'Output'),
                                  Tab(text: 'Logs'),
                                  Tab(text: 'Watch'),
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
                                            height: 280,
                                            child: JsonEditor(value: pretty(detail.run.outputJson), onChanged: (_) {}, readOnly: true),
                                          ),
                                        ],
                                      ),
                                    ),
                                    _loadingLogs
                                        ? const Center(child: CircularProgressIndicator())
                                        : LogPanel(chunks: _nodeChunks),
                                    const SingleChildScrollView(padding: EdgeInsets.all(12), child: WatchExpressionsPanel()),
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
