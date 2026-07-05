import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/workflows_service.dart';
import '../../core/services/workflows/state.dart';
import '../theme/app_theme.dart';

class RunTabsBar extends ConsumerWidget {
  const RunTabsBar({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final workflows = ref.watch(workflowsProvider);
    final notifier = ref.read(workflowsProvider.notifier);

    if (workflows.openRunIds.isEmpty) return const SizedBox.shrink();

    return Container(
      decoration: BoxDecoration(
        color: AppColors.surfaceSubtle,
        border: Border(bottom: BorderSide(color: AppColors.border)),
      ),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.fromLTRB(6, 4, 6, 0),
        child: Row(
          children: [
            for (final runId in workflows.openRunIds)
              _RunTab(
                runId: runId,
                active: runId == workflows.selectedWorkflowRunId,
                status: _statusFor(workflows, runId),
                label: _labelFor(workflows, runId),
                onTap: () => notifier.runs.activateRunTab(runId),
                onClose: () => notifier.runs.closeRunTab(runId),
              ),
          ],
        ),
      ),
    );
  }

  String _labelFor(WorkflowServicesState workflows, String runId) {
    final summary = workflows.workflowRuns.where((run) => run.id == runId).firstOrNull;
    final name = summary?.name?.trim();
    return (name != null && name.isNotEmpty) ? name : 'Run #$runId';
  }

  String? _statusFor(WorkflowServicesState workflows, String runId) {
    if (workflows.workflowRunDetail?.run.id == runId) {
      return workflows.workflowRunDetail?.run.status;
    }
    return workflows.workflowRuns.where((run) => run.id == runId).firstOrNull?.status;
  }
}

class _RunTab extends StatelessWidget {
  const _RunTab({
    required this.runId,
    required this.active,
    required this.status,
    required this.label,
    required this.onTap,
    required this.onClose,
  });

  final String runId;
  final bool active;
  final String? status;
  final String label;
  final VoidCallback onTap;
  final VoidCallback onClose;

  Color get _dotColor {
    switch (status) {
      case 'succeeded':
        return AppColors.successFg;
      case 'failed':
      case 'timed_out':
        return AppColors.dangerFg;
      case 'canceled':
        return AppColors.warningFg;
      case 'running':
      case 'queued':
      case 'debug_paused':
        return AppColors.accent;
      default:
        return AppColors.borderStrong;
    }
  }

  @override
  Widget build(BuildContext context) {
    return Material(
      color: active ? AppColors.surface : Colors.transparent,
      child: InkWell(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
          decoration: BoxDecoration(
            border: Border(
              top: BorderSide(color: AppColors.border),
              left: BorderSide(color: AppColors.border),
              right: BorderSide(color: AppColors.border),
              bottom: active ? BorderSide(color: AppColors.surface, width: 2) : BorderSide.none,
            ),
            borderRadius: const BorderRadius.vertical(top: Radius.circular(6)),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(color: _dotColor, shape: BoxShape.circle),
              ),
              const SizedBox(width: 6),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 180),
                child: Text(label, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12)),
              ),
              IconButton(
                visualDensity: VisualDensity.compact,
                iconSize: 14,
                padding: EdgeInsets.zero,
                constraints: const BoxConstraints(minWidth: 24, minHeight: 24),
                icon: const Icon(Icons.close),
                onPressed: onClose,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
