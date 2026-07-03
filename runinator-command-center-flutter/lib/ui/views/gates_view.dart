import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/gate/gate_record.dart';
import '../../core/services/app_service.dart';
import '../../core/services/gates_service.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../shared/split_pane.dart';

class _AlwaysConfirm implements ConfirmContext {
  @override
  bool confirm(String message) => true;

  @override
  String? prompt(String message) => null;
}

class GatesView extends ConsumerWidget {
  const GatesView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final gatesState = ref.watch(gatesProvider);
    final gates = ref.read(gatesProvider.notifier);
    final app = ref.watch(appProvider);
    final filtered = gates.filteredGates(ref.read(appProvider.notifier).normalizedSearch);
    final selected = gatesState.selectedGate;
    final selectedIndex = selected == null ? null : filtered.indexOf(selected);

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        initialFirstFraction: 0.58,
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: 'Gates',
                actions: [
                  CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => gates.refreshGates()),
                  CcButton(icon: IconName.approve, label: 'Open', variant: CcButtonVariant.primary, dense: true, onPressed: gates.canResolveSelected() ? () => gates.resolveSelected('open') : null),
                  CcButton(icon: IconName.reject, label: 'Close', variant: CcButtonVariant.danger, dense: true, onPressed: gates.canResolveSelected() ? () => gates.resolveSelected('close') : null),
                  CcButton(icon: IconName.trash, label: 'Delete', dense: true, onPressed: selected == null ? null : () => gates.removeSelected(_AlwaysConfirm())),
                ],
              ),
              Expanded(
                child: CcDataTable(
                  columns: const ['Status', 'Kind', 'Label', 'Node', 'Run'],
                  rows: [
                    for (final gate in filtered)
                      [gate.status ?? '', gate.kind.wire, gate.label ?? '', gate.nodeId ?? '', gate.workflowRunId ?? ''],
                  ],
                  selectedIndex: selectedIndex != null && selectedIndex >= 0 ? selectedIndex : null,
                  onSelect: (index) => gates.setSelectedGate(filtered[index]),
                  emptyMessage: gatesState.gates.isEmpty ? 'No gates are currently blocking a workflow.' : 'No gates match "${app.searchQuery}".',
                ),
              ),
            ],
          ),
        ),
        second: selected == null
            ? const EmptyState(message: 'Select a gate to inspect.')
            : PanelCard(child: JsonEditor(value: selected.toJson().toString(), onChanged: (_) {}, readOnly: true)),
      ),
    );
  }
}

extension on GateRecord {
  Map<String, Object?> toJson() => {
        'id': id,
        'status': status,
        'kind': kind.wire,
        'label': label,
        'node_id': nodeId,
        'workflow_run_id': workflowRunId,
      };
}
