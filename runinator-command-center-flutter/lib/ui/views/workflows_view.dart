import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/app_service.dart';
import '../../core/services/workflows_service.dart';
import '../adapters/workflow_graph_bridge.dart';
import '../shared/cc_widgets.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';
import '../workflow/step_editor_modal.dart';
import '../workflow/wdl_editor_panel.dart';
import '../workflow/workflow_graph_canvas.dart';
import '../workflow/workflow_modals.dart';

class WorkflowsView extends ConsumerStatefulWidget {
  const WorkflowsView({super.key});

  @override
  ConsumerState<WorkflowsView> createState() => _WorkflowsViewState();
}

class _WorkflowsViewState extends ConsumerState<WorkflowsView> {
  var _shareOpen = false;

  @override
  Widget build(BuildContext context) {
    final workflows = ref.watch(workflowsProvider);
    final notifier = ref.read(workflowsProvider.notifier);
    final app = ref.watch(appProvider);
    final bridge = WorkflowGraphBridge(notifier);
    final filtered = notifier.host.getFilteredWorkflows();
    final selectedId = workflows.selectedWorkflowId;

    return Stack(
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: SplitPane(
            initialFirstFraction: 0.22,
            minFirst: 220,
            minSecond: 480,
            first: PanelCard(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  PanelToolbar(
                    title: 'Workflows',
                    actions: [
                      CcButton(icon: IconName.plus, label: 'New', dense: true, onPressed: () => notifier.catalog.addWorkflow()),
                      CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.catalog.refreshWorkflows()),
                    ],
                  ),
                  Expanded(
                    child: filtered.isEmpty
                        ? EmptyState(message: app.searchQuery.isEmpty ? 'No workflows yet.' : 'No workflows match "${app.searchQuery}".')
                        : ListView.builder(
                            itemCount: filtered.length,
                            itemBuilder: (context, index) {
                              final workflow = filtered[index];
                              final selected = workflow.id == selectedId;
                              return ListTile(
                                selected: selected,
                                title: Text(workflow.name, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600)),
                                subtitle: Text(workflow.version, style: const TextStyle(fontSize: 11)),
                                onTap: () => notifier.catalog.selectWorkflow(workflow),
                              );
                            },
                          ),
                  ),
                ],
              ),
            ),
            second: selectedId == null
                ? const EmptyState(message: 'Select a workflow to edit.')
                : SplitPane(
                    initialFirstFraction: 0.58,
                    first: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        PanelToolbar(
                          title: workflows.workflowDraft.name,
                          actions: [
                            CcButton(icon: IconName.save, label: 'Save', variant: CcButtonVariant.primary, dense: true, onPressed: () => notifier.catalog.saveSelectedWorkflowBundle()),
                            CcButton(icon: IconName.play, label: 'Run', dense: true, onPressed: () => notifier.runs.runSelectedWorkflow()),
                            CcButton(icon: IconName.settings, label: 'Settings', dense: true, onPressed: () => notifier.catalog.openWorkflowSettings()),
                            CcButton(icon: IconName.key, label: 'Share', dense: true, onPressed: () => setState(() => _shareOpen = true)),
                            MenuAnchor(
                              menuChildren: [
                                for (final kind in notifier.nodeKinds)
                                  MenuItemButton(onPressed: () => notifier.editor.addWorkflowNode(kind), child: Text('Add $kind')),
                              ],
                              builder: (context, controller, child) => CcButton(
                                icon: IconName.plus,
                                label: 'Node',
                                dense: true,
                                onPressed: () => controller.isOpen ? controller.close() : controller.open(),
                              ),
                            ),
                            CcButton(icon: IconName.grid, label: 'Arrange', dense: true, onPressed: () => notifier.editor.autoArrangeWorkflowNodes()),
                          ],
                        ),
                        Expanded(
                          child: Padding(
                            padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
                            child: WorkflowGraphCanvas(
                              nodes: bridge.nodes,
                              edges: bridge.edges,
                              selectedNodeId: workflows.selectedStepId.isEmpty ? null : workflows.selectedStepId,
                              selectedEdgeId: workflows.selectedGraphEdgeId.isEmpty ? null : workflows.selectedGraphEdgeId,
                              onNodeClick: bridge.onNodeClick,
                              onNodeDoubleClick: bridge.onNodeDoubleClick,
                              onNodeDragEnd: bridge.onNodeDragEnd,
                              onEdgeClick: bridge.onEdgeClick,
                              onPaneClick: bridge.clearSelection,
                            ),
                          ),
                        ),
                      ],
                    ),
                    second: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (workflows.workflowWdlError.isNotEmpty)
                          Container(
                            margin: const EdgeInsets.all(12),
                            padding: const EdgeInsets.all(10),
                            decoration: BoxDecoration(color: AppColors.dangerBg, borderRadius: BorderRadius.circular(6)),
                            child: Text(workflows.workflowWdlError, style: const TextStyle(color: AppColors.dangerFg, fontSize: 12)),
                          ),
                        Expanded(
                          child: Padding(
                            padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
                            child: WdlEditorPanel(
                              value: workflows.workflowWdl,
                              readOnly: workflows.workflowWdlError.isNotEmpty,
                              onChanged: (value) {
                                notifier.editor.setWorkflowWdlSilently(value);
                                notifier.editor.scheduleWorkflowWdlSync();
                              },
                            ),
                          ),
                        ),
                        if (workflows.selectedStepId.isNotEmpty)
                          _StepInspector(selectedStepId: workflows.selectedStepId, notifier: notifier),
                      ],
                    ),
                  ),
          ),
        ),
        const RunInputModal(),
        const WorkflowSettingsModal(),
        const StepEditorModal(),
        if (_shareOpen && selectedId != null)
          ShareWorkflowModal(workflowId: selectedId, onClose: () => setState(() => _shareOpen = false)),
      ],
    );
  }
}

class _StepInspector extends StatelessWidget {
  const _StepInspector({required this.selectedStepId, required this.notifier});

  final String selectedStepId;
  final WorkflowsNotifier notifier;

  @override
  Widget build(BuildContext context) {
    final node = notifier.host.getSelectedNode();
    if (node == null) return const SizedBox.shrink();

    return Container(
      margin: const EdgeInsets.all(12),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(color: AppColors.surfaceSubtle, borderRadius: BorderRadius.circular(8), border: Border.all(color: AppColors.border)),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(node['name']?.toString() ?? selectedStepId, style: const TextStyle(fontWeight: FontWeight.w700)),
                Text('${node['kind']} · $selectedStepId', style: const TextStyle(fontSize: 11, color: AppColors.textMuted)),
              ],
            ),
          ),
          CcButton(icon: IconName.edit, label: 'Edit', dense: true, onPressed: () => notifier.editor.openStepEditor(selectedStepId)),
          const SizedBox(width: 8),
          CcButton(icon: IconName.trash, label: 'Remove', variant: CcButtonVariant.danger, dense: true, onPressed: notifier.host.canRemoveSelectedStep() ? () => notifier.editor.removeWorkflowNode(selectedStepId) : null),
        ],
      ),
    );
  }
}
