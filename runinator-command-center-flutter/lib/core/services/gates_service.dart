// port of core/services/gates.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import 'app_service.dart';

part 'gates_service.g.dart';

class GatesState {
  const GatesState({required this.gates, this.selectedGate});

  final List<GateRecord> gates;
  final GateRecord? selectedGate;

  GatesState copyWith({List<GateRecord>? gates, Object? selectedGate = _unset}) => GatesState(
        gates: gates ?? this.gates,
        selectedGate: identical(selectedGate, _unset) ? this.selectedGate : selectedGate as GateRecord?,
      );
}

const Object _unset = Object();

/// caller-supplied confirm/prompt surface; the future UI pass supplies a concrete
/// implementation (mirrors the ts source's ConfirmContext).
abstract class ConfirmContext {
  bool confirm(String message);

  String? prompt(String message);
}

@riverpod
class GatesNotifier extends _$GatesNotifier {
  @override
  GatesState build() => const GatesState(gates: [], selectedGate: null);

  List<GateRecord> filteredGates(String query) {
    if (query.isEmpty) {
      return state.gates;
    }

    final matches = <GateRecord>[];

    for (final gate in state.gates) {
      final haystack = [gate.id, gate.kind.wire, gate.status, gate.label, gate.nodeId, gate.workflowRunId]
          .where((value) => value != null)
          .map((value) => value.toString().toLowerCase());

      if (haystack.any((value) => value.contains(query))) {
        matches.add(gate);
      }
    }

    return matches;
  }

  bool canResolveSelected() {
    final status = state.selectedGate?.status ?? '';
    return (state.selectedGate?.id ?? '').isNotEmpty && ['pending', 'closed'].contains(status);
  }

  void setSelectedGate(GateRecord? gate) {
    state = state.copyWith(selectedGate: gate);
  }

  Future<void> refreshGates() async {
    List<GateRecord> gates;
    try {
      gates = await ref.read(appProvider.notifier).runOperation('Refreshing gates', api.fetchGates);
    } catch (_) {
      gates = [];
    }

    final selectedId = state.selectedGate?.id;
    state = state.copyWith(
      gates: gates,
      selectedGate: gates.where((gate) => gate.id == selectedId).isNotEmpty
          ? gates.firstWhere((gate) => gate.id == selectedId)
          : (gates.isNotEmpty ? gates.first : null),
    );
  }

  void clearGates() {
    state = const GatesState(gates: [], selectedGate: null);
  }

  Future<void> resolveSelected(String action, [String? reason]) async {
    final app = ref.read(appProvider.notifier);
    final gateId = state.selectedGate?.id ?? '';

    if (gateId.isEmpty) {
      app.setError('No gate selected');
      return;
    }

    final trimmed = (reason != null && reason.trim().isNotEmpty) ? reason.trim() : null;
    final response = await app.runOperation(
      action == 'open' ? 'Opening gate' : 'Closing gate',
      () => action == 'open' ? api.openGate(gateId, trimmed) : api.closeGate(gateId, trimmed),
    );
    app.setStatus(response.message);
    await refreshGates();
  }

  Future<void> removeSelected(ConfirmContext confirm) async {
    final app = ref.read(appProvider.notifier);
    final gateId = state.selectedGate?.id ?? '';

    if (gateId.isEmpty) {
      app.setError('No gate selected');
      return;
    }

    if (!confirm.confirm('Delete this gate record?')) {
      return;
    }

    try {
      await app.runOperation('Deleting gate', () => api.deleteGate(gateId));
    } catch (error) {
      app.setError(error.toString());
    }

    state = state.copyWith(
      gates: state.gates.where((gate) => gate.id != gateId).toList(),
      selectedGate: state.gates.where((gate) => gate.id != gateId).isNotEmpty
          ? state.gates.where((gate) => gate.id != gateId).first
          : null,
    );
    await refreshGates();
  }
}
