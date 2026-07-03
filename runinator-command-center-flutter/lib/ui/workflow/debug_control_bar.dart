import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/api/command_center_api.dart';
import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/domain/models/workflow_state/debug_mode.dart';
import '../../core/services/workflows/state.dart';
import '../../core/services/workflows_service.dart';
import '../shared/cc_widgets.dart';
import 'debug_json_modal.dart';

class DebugControlBar extends ConsumerStatefulWidget {
  const DebugControlBar({super.key});

  @override
  ConsumerState<DebugControlBar> createState() => _DebugControlBarState();
}

class _DebugControlBarState extends ConsumerState<DebugControlBar> {
  var _skipOpen = false;
  var _rerunOpen = false;

  WorkflowNodeRun? _currentNode(WorkflowServicesState workflows) {
    final nodeId = workflows.selectedWorkflowRunNodeId;
    for (final node in workflows.workflowRunDetail?.nodes ?? const <WorkflowNodeRun>[]) {
      if (node.nodeId == nodeId) return node;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    final workflows = ref.watch(workflowsProvider);
    final notifier = ref.read(workflowsProvider.notifier);
    final host = notifier.host;
    final debug = host.getDebugState();
    final mode = debug?.mode?.wire ?? DebugMode.stepAll.wire;
    final canRunToCursor = host.canContinueWorkflowRun() && workflows.selectedWorkflowRunNodeId.isNotEmpty;
    final currentNode = _currentNode(workflows);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            CcButton(icon: IconName.continue_, label: 'Continue', variant: CcButtonVariant.primary, dense: true, onPressed: host.canContinueWorkflowRun() ? () => notifier.runs.continueSelectedWorkflowRun() : null),
            CcButton(icon: IconName.step, label: 'Step', dense: true, onPressed: host.canStepWorkflowRun() ? () => notifier.runs.stepSelectedWorkflowRun() : null),
            CcButton(icon: IconName.cursor, label: 'To cursor', dense: true, onPressed: canRunToCursor ? () => notifier.runs.runToCursor(workflows.selectedWorkflowRunNodeId) : null),
            CcButton(icon: IconName.skip, label: 'Skip', dense: true, onPressed: host.canStepWorkflowRun() ? () => setState(() => _skipOpen = true) : null),
            CcButton(icon: IconName.replay, label: 'Re-run', dense: true, onPressed: host.canStepWorkflowRun() ? () => setState(() => _rerunOpen = true) : null),
          ],
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 16,
          children: [
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Radio<String>(
                  value: DebugMode.stepAll.wire,
                  groupValue: mode,
                  onChanged: host.isDebugRun()
                      ? (v) {
                          if (v != null) {
                            notifier.runs.patchSelectedWorkflowRunDebug(WorkflowDebugPatch(mode: DebugMode.stepAll));
                          }
                        }
                      : null,
                ),
                const Text('Pause every node', style: TextStyle(fontSize: 12)),
              ],
            ),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Radio<String>(
                  value: DebugMode.breakpoints.wire,
                  groupValue: mode,
                  onChanged: host.isDebugRun()
                      ? (v) {
                          if (v != null) {
                            notifier.runs.patchSelectedWorkflowRunDebug(WorkflowDebugPatch(mode: DebugMode.breakpoints));
                          }
                        }
                      : null,
                ),
                Text('Pause at breakpoints only (${host.getCurrentBreakpoints().length})', style: const TextStyle(fontSize: 12)),
              ],
            ),
          ],
        ),
        if (_skipOpen)
          DebugJsonModal(
            title: 'Skip current node',
            hint: 'Provide synthetic output to record for this node.',
            submitLabel: 'Skip with this output',
            initialValue: currentNode?.outputJson ?? {},
            onClose: () => setState(() => _skipOpen = false),
            onSubmit: (value) async {
              setState(() => _skipOpen = false);
              await notifier.runs.skipCurrentNode(value);
            },
          ),
        if (_rerunOpen)
          DebugJsonModal(
            title: 'Re-run current node',
            hint: 'Modify the parameters and re-run the current node.',
            submitLabel: 'Re-run',
            initialValue: currentNode?.parameters ?? {},
            onClose: () => setState(() => _rerunOpen = false),
            onSubmit: (value) async {
              setState(() => _rerunOpen = false);
              await notifier.runs.rerunCurrentNode(value);
            },
          ),
      ],
    );
  }
}
