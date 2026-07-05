import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/workflows_service.dart';
import '../../core/workflow/graph_model.dart';
import '../adapters/workflow_graph_bridge.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';
import 'workflow_graph_canvas.dart';

class WorkflowEditorShell extends ConsumerStatefulWidget {
  const WorkflowEditorShell({super.key, required this.bridge});

  final WorkflowGraphBridge bridge;

  @override
  ConsumerState<WorkflowEditorShell> createState() => _WorkflowEditorShellState();
}

class _WorkflowEditorShellState extends ConsumerState<WorkflowEditorShell> {
  Offset? _menuPosition;
  _ContextMenuKind? _menuKind;
  String? _menuTargetId;
  bool _menuDeletable = true;
  Offset? _connectMenuPosition;
  String? _connectSourceId;
  String? _connectTargetId;
  List<WorkflowEdgeSemanticOption>? _connectOptions;
  WorkflowEdgeEditorDraft? _edgeEditor;

  WorkflowsNotifier get _notifier => ref.read(workflowsProvider.notifier);

  void _closeOverlays() {
    setState(() {
      _menuPosition = null;
      _connectMenuPosition = null;
      _edgeEditor = null;
    });
  }

  void _openNodeMenu(String nodeId, Offset position, {required bool deletable}) {
    setState(() {
      _menuPosition = position;
      _menuKind = _ContextMenuKind.node;
      _menuTargetId = nodeId;
      _menuDeletable = deletable;
      _connectMenuPosition = null;
      _edgeEditor = null;
    });
  }

  void _openEdgeMenu(String edgeId, Offset position) {
    setState(() {
      _menuPosition = position;
      _menuKind = _ContextMenuKind.edge;
      _menuTargetId = edgeId;
      _connectMenuPosition = null;
      _edgeEditor = null;
    });
  }

  void _openConnectMenu(String sourceId, String targetId, List<WorkflowEdgeSemanticOption> options, Offset position) {
    if (options.length == 1) {
      _notifier.editor.applyGraphEdgeSemantic(
        GraphEdgeLike(source: sourceId, target: targetId),
        options.first.id,
      );
      _notifier.host.notify();
      return;
    }

    setState(() {
      _connectSourceId = sourceId;
      _connectTargetId = targetId;
      _connectOptions = options;
      _connectMenuPosition = position;
      _menuPosition = null;
      _edgeEditor = null;
    });
  }

  void _openEdgeEditor(String edgeId) {
    final draft = _notifier.editor.openEdgeEditorDraft(edgeId);
    if (draft == null) return;
    setState(() {
      _edgeEditor = draft;
      _menuPosition = null;
      _connectMenuPosition = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final workflows = ref.watch(workflowsProvider);
    final bridge = widget.bridge;
    final issues = _notifier.host.getGraphValidationIssues();
    final selectedNode = workflows.selectedStepId.isNotEmpty
        ? bridge.nodes.where((n) => n.id == workflows.selectedStepId).firstOrNull
        : null;
    final selectedEdge = workflows.selectedGraphEdgeId.isNotEmpty
        ? bridge.edges.where((e) => e.id == workflows.selectedGraphEdgeId).firstOrNull
        : null;

    return Stack(
      clipBehavior: Clip.none,
      children: [
        Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              child: WorkflowGraphCanvas(
                nodes: bridge.nodes,
                edges: bridge.edges,
                selectedNodeId: workflows.selectedStepId.isEmpty ? null : workflows.selectedStepId,
                selectedEdgeId: workflows.selectedGraphEdgeId.isEmpty ? null : workflows.selectedGraphEdgeId,
                onNodeClick: bridge.onNodeClick,
                onNodeDoubleClick: bridge.onNodeDoubleClick,
                onNodeDragEnd: bridge.onNodeDragEnd,
                onEdgeClick: (edgeId) {
                  bridge.onEdgeClick(edgeId);
                  setState(() {});
                },
                onPaneClick: () {
                  bridge.clearSelection();
                  _closeOverlays();
                },
                onNodeContextMenu: (nodeId, position, {required bool deletable}) =>
                    _openNodeMenu(nodeId, position, deletable: deletable),
                onEdgeContextMenu: (edgeId, position) => _openEdgeMenu(edgeId, position),
                onConnect: (sourceId, targetId, position) {
                  final options = _notifier.editor.workflowEdgeOptions(sourceId);
                  if (options.isEmpty) return;
                  _openConnectMenu(sourceId, targetId, options, position);
                },
                // dragged from a specific per-parameter handle, so the route is already
                // known — apply it directly instead of opening the "connect as" picker.
                onConnectWithOption: (sourceId, targetId, optionId, position) {
                  _notifier.editor.applyGraphEdgeSemantic(
                    GraphEdgeLike(source: sourceId, target: targetId),
                    optionId,
                  );
                  _notifier.host.notify();
                  setState(() {});
                },
              ),
            ),
            _WorkflowDiagnosticsPanel(issues: issues, onFocus: (nodeId) {
              bridge.onNodeClick(nodeId);
              setState(() {});
            }),
          ],
        ),
        if (selectedNode != null || selectedEdge != null)
          Positioned(
            left: 12,
            right: 12,
            bottom: issues.isEmpty ? 12 : 140,
            child: _WorkflowCommandBar(
              selectedNode: selectedNode,
              selectedEdge: selectedEdge,
              onEditNode: () => _notifier.editor.openStepEditor(workflows.selectedStepId),
              onDuplicateNode: _notifier.host.canRemoveSelectedStep() ? () => _notifier.editor.duplicateSelectedStep() : null,
              onDeleteNode: _notifier.host.canRemoveSelectedStep() ? () => _notifier.editor.removeWorkflowStep() : null,
              onAddConnected: () => _notifier.editor.addConnectedWorkflowNode('action'),
              onAutoArrange: () => _notifier.editor.autoArrangeWorkflowNodes(),
              onEditEdge: selectedEdge != null && selectedEdge.id != null ? () => _openEdgeEditor(selectedEdge.id!) : null,
              onDeleteEdge: selectedEdge != null && selectedEdge.id != null
                  ? () {
                      _notifier.editor.removeWorkflowEdgeById(selectedEdge.id!);
                _notifier.host.notify();
                setState(() {});
              } : null,
              onReverseEdge: selectedEdge != null ? () {
                _notifier.editor.reverseSelectedEdgeHandles();
                setState(() {});
              } : null,
              onMoveEdgeUp: selectedEdge != null ? () {
                _notifier.editor.moveSelectedEdge(-1);
                setState(() {});
              } : null,
              onMoveEdgeDown: selectedEdge != null ? () {
                _notifier.editor.moveSelectedEdge(1);
                setState(() {});
              } : null,
              nodeIssue: selectedNode?.data.validationIssues.firstOrNull?.message,
              edgeIssue: selectedEdge?.data.validationMessages?.firstOrNull,
            ),
          ),
        if (_menuPosition != null && _menuKind != null && _menuTargetId != null)
          _ContextMenuOverlay(
            position: _menuPosition!,
            items: _menuKind == _ContextMenuKind.node
                ? [
                    _MenuAction('Edit', () {
                      _notifier.editor.openStepEditor(_menuTargetId!);
                      _closeOverlays();
                    }),
                    if (_menuDeletable)
                      _MenuAction('Delete', () {
                        _notifier.editor.removeWorkflowNode(_menuTargetId!);
                        _notifier.host.notify();
                        _closeOverlays();
                      }),
                    _MenuAction('Add connected node', () {
                      _notifier.editor.addConnectedWorkflowNode('action');
                      _closeOverlays();
                    }),
                  ]
                : [
                    _MenuAction('Edit edge', () {
                      _openEdgeEditor(_menuTargetId!);
                    }),
                    _MenuAction('Delete edge', () {
                      _notifier.editor.removeWorkflowEdgeById(_menuTargetId!);
                      _notifier.host.notify();
                      _closeOverlays();
                    }),
                  ],
            onDismiss: _closeOverlays,
          ),
        if (_connectMenuPosition != null && _connectOptions != null && _connectSourceId != null && _connectTargetId != null)
          _ContextMenuOverlay(
            position: _connectMenuPosition!,
            title: 'Connect as',
            items: [
              for (final option in _connectOptions!)
                _MenuAction(option.label, () {
                  _notifier.editor.applyGraphEdgeSemantic(
                    GraphEdgeLike(source: _connectSourceId!, target: _connectTargetId!),
                    option.id,
                  );
                  _notifier.host.notify();
                  _closeOverlays();
                }),
            ],
            onDismiss: _closeOverlays,
          ),
        if (_edgeEditor != null)
          _EdgeEditorDialog(
            draft: _edgeEditor!,
            onClose: _closeOverlays,
            onApply: (draft) {
              if (_notifier.editor.applyEdgeEditorDraft(draft)) {
                _closeOverlays();
                setState(() {});
              }
            },
            onMove: (direction) {
              final moved = _notifier.editor.moveEdgeEditorItem(_edgeEditor!, direction);
              if (moved != null) setState(() => _edgeEditor = moved);
            },
          ),
      ],
    );
  }
}

enum _ContextMenuKind { node, edge }

class _MenuAction {
  const _MenuAction(this.label, this.onTap);

  final String label;
  final VoidCallback onTap;
}

class _ContextMenuOverlay extends StatelessWidget {
  const _ContextMenuOverlay({required this.position, required this.items, required this.onDismiss, this.title});

  final Offset position;
  final List<_MenuAction> items;
  final VoidCallback onDismiss;
  final String? title;

  @override
  Widget build(BuildContext context) {
    return Stack(
      children: [
        Positioned.fill(
          child: GestureDetector(onTap: onDismiss, behavior: HitTestBehavior.translucent),
        ),
        Positioned(
          left: position.dx.clamp(8, MediaQuery.sizeOf(context).width - 220),
          top: position.dy.clamp(8, MediaQuery.sizeOf(context).height - 200),
          child: Material(
            elevation: 8,
            borderRadius: BorderRadius.circular(8),
            child: ConstrainedBox(
              constraints: const BoxConstraints(minWidth: 180, maxWidth: 260),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (title != null)
                    Padding(
                      padding: const EdgeInsets.fromLTRB(12, 10, 12, 4),
                      child: Text(title!, style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 12)),
                    ),
                  for (final item in items)
                    InkWell(
                      onTap: item.onTap,
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                        child: Text(item.label, style: const TextStyle(fontSize: 13)),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _WorkflowCommandBar extends StatelessWidget {
  const _WorkflowCommandBar({
    required this.selectedNode,
    required this.selectedEdge,
    this.onEditNode,
    this.onDuplicateNode,
    this.onDeleteNode,
    this.onAddConnected,
    this.onAutoArrange,
    this.onEditEdge,
    this.onDeleteEdge,
    this.onReverseEdge,
    this.onMoveEdgeUp,
    this.onMoveEdgeDown,
    this.nodeIssue,
    this.edgeIssue,
  });

  final GraphNodeModel? selectedNode;
  final GraphEdgeModel? selectedEdge;
  final VoidCallback? onEditNode;
  final VoidCallback? onDuplicateNode;
  final VoidCallback? onDeleteNode;
  final VoidCallback? onAddConnected;
  final VoidCallback? onAutoArrange;
  final VoidCallback? onEditEdge;
  final VoidCallback? onDeleteEdge;
  final VoidCallback? onReverseEdge;
  final VoidCallback? onMoveEdgeUp;
  final VoidCallback? onMoveEdgeDown;
  final String? nodeIssue;
  final String? edgeIssue;

  @override
  Widget build(BuildContext context) {
    return Material(
      elevation: 4,
      borderRadius: BorderRadius.circular(8),
      color: AppColors.surface,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
        child: Wrap(
          spacing: 8,
          runSpacing: 6,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            if (selectedEdge != null) ...[
              CcButton(icon: IconName.edit, label: 'Edit', dense: true, onPressed: onEditEdge),
              CcButton(icon: IconName.trash, label: 'Delete', dense: true, onPressed: onDeleteEdge),
              CcButton(icon: IconName.refresh, label: 'Reverse', dense: true, onPressed: onReverseEdge),
              CcButton(icon: IconName.arrowUp, label: 'Up', dense: true, onPressed: onMoveEdgeUp),
              CcButton(icon: IconName.arrowDown, label: 'Down', dense: true, onPressed: onMoveEdgeDown),
              if (edgeIssue != null) Text(edgeIssue!, style: TextStyle(fontSize: 11, color: AppColors.dangerFg)),
            ] else if (selectedNode != null) ...[
              CcButton(icon: IconName.edit, label: 'Edit', dense: true, onPressed: onEditNode),
              CcButton(icon: IconName.edit, label: 'Duplicate', dense: true, onPressed: onDuplicateNode),
              CcButton(icon: IconName.trash, label: 'Delete', dense: true, variant: CcButtonVariant.danger, onPressed: onDeleteNode),
              CcButton(icon: IconName.plus, label: 'Add node', dense: true, onPressed: onAddConnected),
              CcButton(icon: IconName.grid, label: 'Arrange', dense: true, onPressed: onAutoArrange),
              if (nodeIssue != null) Text(nodeIssue!, style: TextStyle(fontSize: 11, color: AppColors.dangerFg)),
            ],
          ],
        ),
      ),
    );
  }
}

class _WorkflowDiagnosticsPanel extends StatelessWidget {
  const _WorkflowDiagnosticsPanel({required this.issues, required this.onFocus});

  final List<WorkflowValidationIssue> issues;
  final ValueChanged<String> onFocus;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 120,
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: AppColors.border)),
        color: AppColors.surfaceSubtle,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
            child: Text('Diagnostics (${issues.length})', style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 12)),
          ),
          Expanded(
            child: issues.isEmpty
                ? Center(child: Text('No workflow issues.', style: TextStyle(fontSize: 12, color: AppColors.textMuted)))
                : ListView.builder(
                    itemCount: issues.length,
                    itemBuilder: (context, index) {
                      final issue = issues[index];
                      return ListTile(
                        dense: true,
                        title: Text(issue.message, style: const TextStyle(fontSize: 12)),
                        subtitle: Text('${issue.severity.name} · ${issue.nodeId}', style: const TextStyle(fontSize: 10)),
                        onTap: issue.nodeId.isNotEmpty ? () => onFocus(issue.nodeId) : null,
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}

class _EdgeEditorDialog extends StatefulWidget {
  const _EdgeEditorDialog({required this.draft, required this.onClose, required this.onApply, required this.onMove});

  final WorkflowEdgeEditorDraft draft;
  final VoidCallback onClose;
  final ValueChanged<WorkflowEdgeEditorDraft> onApply;
  final ValueChanged<int> onMove;

  @override
  State<_EdgeEditorDialog> createState() => _EdgeEditorDialogState();
}

class _EdgeEditorDialogState extends State<_EdgeEditorDialog> {
  late WorkflowEdgeEditorDraft _draft;
  late TextEditingController _labelController;
  late TextEditingController _whenController;
  late TextEditingController _matchController;

  @override
  void initState() {
    super.initState();
    _draft = widget.draft;
    _labelController = TextEditingController(text: _draft.label);
    _whenController = TextEditingController(text: _draft.whenJson);
    _matchController = TextEditingController(text: _draft.matchJson);
  }

  @override
  void dispose() {
    _labelController.dispose();
    _whenController.dispose();
    _matchController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Stack(
      children: [
        Positioned.fill(child: GestureDetector(onTap: widget.onClose)),
        Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: Material(
              elevation: 12,
              borderRadius: BorderRadius.circular(8),
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text('Edge ${_draft.source} → ${_draft.target}', style: const TextStyle(fontWeight: FontWeight.w700)),
                    const SizedBox(height: 12),
                    if (_draft.canEditLabel)
                      TextField(
                        decoration: const InputDecoration(labelText: 'Label', isDense: true),
                        controller: _labelController,
                        onChanged: (v) => setState(() => _draft = _draft.copyWith(label: v)),
                      ),
                    if (_draft.canEditCondition) ...[
                      const SizedBox(height: 8),
                      TextField(
                        decoration: const InputDecoration(labelText: 'When (JSON)', isDense: true),
                        controller: _whenController,
                        maxLines: 3,
                        onChanged: (v) => setState(() => _draft = _draft.copyWith(whenJson: v)),
                      ),
                    ],
                    if (_draft.canEditSwitchCase) ...[
                      const SizedBox(height: 8),
                      TextField(
                        decoration: const InputDecoration(labelText: 'Match (JSON)', isDense: true),
                        controller: _matchController,
                        maxLines: 3,
                        onChanged: (v) => setState(() => _draft = _draft.copyWith(matchJson: v)),
                      ),
                    ],
                    const SizedBox(height: 12),
                    Row(
                      children: [
                        if (_draft.canMove) ...[
                          CcButton(icon: IconName.arrowUp, label: 'Up', dense: true, onPressed: () => widget.onMove(-1)),
                          const SizedBox(width: 8),
                          CcButton(icon: IconName.arrowDown, label: 'Down', dense: true, onPressed: () => widget.onMove(1)),
                          const Spacer(),
                        ] else
                          const Spacer(),
                        CcButton(icon: IconName.reject, label: 'Cancel', dense: true, onPressed: widget.onClose),
                        const SizedBox(width: 8),
                        CcButton(icon: IconName.save, label: 'Apply', dense: true, variant: CcButtonVariant.primary, onPressed: () => widget.onApply(_draft)),
                      ],
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

extension _FirstOrNull<E> on Iterable<E> {
  E? get firstOrNull {
    final it = iterator;
    if (!it.moveNext()) return null;
    return it.current;
  }
}
