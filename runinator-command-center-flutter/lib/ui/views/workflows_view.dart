import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/app_service.dart';
import '../../core/services/orgs_service.dart';
import '../../core/services/workflows_service.dart';
import '../adapters/workflow_graph_bridge.dart';
import '../shared/cc_widgets.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';
import '../workflow/step_editor_modal.dart';
import '../workflow/wdl_editor_panel.dart';
import '../workflow/workflow_editor_shell.dart';
import '../workflow/workflow_modals.dart';

class WorkflowsView extends ConsumerStatefulWidget {
  const WorkflowsView({super.key});

  @override
  ConsumerState<WorkflowsView> createState() => _WorkflowsViewState();
}

class _WorkflowsViewState extends ConsumerState<WorkflowsView> {
  var _shareOpen = false;
  var _scopeFilter = 'all';

  Future<bool> _confirmDiscardIfDirty() async {
    if (!ref.read(workflowsProvider).isDirty) return true;
    final result = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Discard changes?'),
        content: const Text('You have unsaved changes to this workflow. Discard them?'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          TextButton(onPressed: () => Navigator.pop(context, true), child: const Text('Discard')),
        ],
      ),
    );
    return result ?? false;
  }

  @override
  Widget build(BuildContext context) {
    final workflows = ref.watch(workflowsProvider);
    final notifier = ref.read(workflowsProvider.notifier);
    final orgs = ref.watch(orgsProvider);
    final filtered = notifier.host.getScopedWorkflows(scopeFilter: _scopeFilter, activeOrgId: orgs.activeOrgId);
    final selectedId = workflows.selectedWorkflowId;
    final bridge = WorkflowGraphBridge(notifier);
    final isActiveDebugRun = notifier.host.isDebugRun() &&
        workflows.workflowRunDetail != null &&
        !['succeeded', 'failed', 'canceled', 'timed_out'].contains(workflows.workflowRunDetail!.run.status);

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
                      CcButton(icon: IconName.plus, label: 'New', dense: true, onPressed: () async {
                        if (await _confirmDiscardIfDirty()) notifier.catalog.addWorkflow();
                      }),
                      CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.catalog.refreshWorkflows()),
                    ],
                  ),
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 12),
                    child: DropdownButtonFormField<String>(
                      value: _scopeFilter,
                      decoration: const InputDecoration(labelText: 'Scope', isDense: true),
                      items: const [
                        DropdownMenuItem(value: 'all', child: Text('All')),
                        DropdownMenuItem(value: 'org', child: Text('This org')),
                        DropdownMenuItem(value: 'global', child: Text('Global')),
                      ],
                      onChanged: (value) {
                        if (value != null) setState(() => _scopeFilter = value);
                      },
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(12, 4, 12, 0),
                    child: Text(
                      '${filtered.length} visible · ${filtered.where((w) => !w.enabled).length} disabled',
                      style: TextStyle(fontSize: 11, color: AppColors.textMuted),
                    ),
                  ),
                  Expanded(
                    child: filtered.isEmpty
                        ? EmptyState(message: ref.read(appProvider.notifier).normalizedSearch.isEmpty ? 'No workflows yet.' : 'No workflows match the current filters.')
                        : ListView.builder(
                            itemCount: filtered.length,
                            itemBuilder: (context, index) {
                              final workflow = filtered[index];
                              final selected = workflow.id == selectedId;
                              return ListTile(
                                selected: selected,
                                title: Text(workflow.name, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600)),
                                subtitle: Text('v${workflow.version}${workflow.orgId == null ? ' · global' : ''}', style: const TextStyle(fontSize: 11)),
                                onTap: () async {
                                  if (workflow.id == selectedId) return;
                                  if (!await _confirmDiscardIfDirty()) return;
                                  notifier.catalog.selectWorkflow(workflow);
                                },
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
                            if (workflows.isDirty) ...[
                              Container(
                                width: 8,
                                height: 8,
                                margin: const EdgeInsets.only(right: 6),
                                decoration: BoxDecoration(color: AppColors.warningFg, shape: BoxShape.circle),
                              ),
                              Padding(
                                padding: const EdgeInsets.only(right: 8),
                                child: Text('Unsaved', style: TextStyle(fontSize: 11, color: AppColors.warningFg)),
                              ),
                            ],
                            CcButton(icon: IconName.save, label: 'Save', variant: CcButtonVariant.primary, dense: true, onPressed: () => notifier.catalog.saveSelectedWorkflowBundle()),
                            CcButton(icon: IconName.play, label: 'Run', dense: true, onPressed: () => notifier.runs.runSelectedWorkflow()),
                            if (!isActiveDebugRun)
                              CcButton(icon: IconName.debug, label: 'Debug', dense: true, onPressed: () => notifier.runs.runSelectedWorkflowDebug())
                            else
                              CcButton(icon: IconName.stop, label: 'Stop debug', variant: CcButtonVariant.danger, dense: true, onPressed: () => notifier.runs.cancelSelectedWorkflowRun()),
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
                            MenuAnchor(
                              menuChildren: [
                                MenuItemButton(onPressed: () => notifier.editor.autoArrangeWorkflowNodes(WorkflowLayoutDirection.horizontal), child: const Text('Left to right')),
                                MenuItemButton(onPressed: () => notifier.editor.autoArrangeWorkflowNodes(WorkflowLayoutDirection.vertical), child: const Text('Top to bottom')),
                              ],
                              builder: (context, controller, child) => CcButton(
                                icon: IconName.grid,
                                label: 'Arrange',
                                dense: true,
                                onPressed: () => controller.isOpen ? controller.close() : controller.open(),
                              ),
                            ),
                            MenuAnchor(
                              menuChildren: [
                                MenuItemButton(onPressed: () => notifier.catalog.exportWorkflowWdl(), child: const Text('This workflow (.wdl)')),
                                MenuItemButton(onPressed: () => notifier.catalog.exportWorkflowPack(), child: const Text('All workflows (.wdlp pack)')),
                              ],
                              builder: (context, controller, child) => CcButton(
                                icon: IconName.download,
                                label: 'Export',
                                dense: true,
                                onPressed: () => controller.isOpen ? controller.close() : controller.open(),
                              ),
                            ),
                          ],
                        ),
                        Expanded(
                          child: Padding(
                            padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
                            child: WorkflowEditorShell(bridge: bridge),
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
                            child: Text(workflows.workflowWdlError, style: TextStyle(color: AppColors.dangerFg, fontSize: 12)),
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
                Text('${node['kind']} · $selectedStepId', style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
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
