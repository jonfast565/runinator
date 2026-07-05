import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/api/command_center_api.dart' as api;
import '../../core/domain/icons.dart';
import '../../core/domain/models/replica.dart';
import '../../core/services/app_service.dart';
import '../../core/services/replica_samples_service.dart';
import '../../core/utils/format.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../shared/sparkline.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';

class ReplicasView extends ConsumerStatefulWidget {
  const ReplicasView({super.key});

  @override
  ConsumerState<ReplicasView> createState() => _ReplicasViewState();
}

class _ReplicasViewState extends ConsumerState<ReplicasView> {
  String? _selectedReplicaId;
  List<api.ReplicaSample> _samples = const [];
  var _samplesLoading = false;

  int _countByStatus(List<ReplicaRecord> replicas, ReplicaStatus status) =>
      replicas.where((r) => r.status == status).length;

  Future<void> _loadSamples(String? replicaId) async {
    if (replicaId == null) {
      setState(() => _samples = const []);
      return;
    }

    setState(() => _samplesLoading = true);
    try {
      final series = await ref.read(replicaSamplesServiceProvider).fetch(replicaId);
      if (mounted) setState(() => _samples = series.samples);
    } catch (_) {
      if (mounted) setState(() => _samples = const []);
    } finally {
      if (mounted) setState(() => _samplesLoading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final app = ref.watch(appProvider);
    final query = ref.read(appProvider.notifier).normalizedSearch;
    final replicas = app.replicas.where((replica) {
      if (query.isEmpty) return true;
      return [
        replica.displayName,
        replica.host,
        replica.instanceId,
        replica.runtimeId,
        replica.observedIp,
        replica.replicaType.name,
        replica.status.name,
        replica.replicaId,
      ].any((v) => displayValue(v).toLowerCase().contains(query));
    }).toList();

    final selected = _selectedReplicaId == null
        ? null
        : replicas.cast<ReplicaRecord?>().firstWhere((r) => r?.replicaId == _selectedReplicaId, orElse: () => null);

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        storageKey: 'command-center.replicas.split',
        initialFirstFraction: 0.38,
        minFirst: 280,
        minSecond: 360,
        mobileShowSecond: selected != null,
        mobileBackTitle: selected?.displayName ?? selected?.host ?? selected?.instanceId,
        onMobileBack: () => setState(() => _selectedReplicaId = null),
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: 'Replicas',
                actions: [
                  CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => ref.read(appProvider.notifier).refreshReplicas()),
                ],
              ),
              Wrap(
                spacing: 12,
                runSpacing: 4,
                children: [
                  Text('${app.replicas.where((r) => r.status == ReplicaStatus.live).length} live', style: const TextStyle(fontSize: 12)),
                  Text('${_countByStatus(app.replicas, ReplicaStatus.stale)} stale', style: TextStyle(fontSize: 12, color: AppColors.warningFg)),
                  Text('${_countByStatus(app.replicas, ReplicaStatus.offline)} offline', style: TextStyle(fontSize: 12, color: AppColors.dangerFg)),
                ],
              ),
              const SizedBox(height: 8),
              Expanded(
                child: replicas.isEmpty
                    ? const EmptyState(message: 'No replicas match the current filters.')
                    : ListView.separated(
                        itemCount: replicas.length,
                        separatorBuilder: (_, __) => const Divider(height: 1),
                        itemBuilder: (context, index) {
                          final replica = replicas[index];
                          final selectedRow = replica.replicaId == _selectedReplicaId;
                          return ListTile(
                            selected: selectedRow,
                            dense: true,
                            title: Text(replica.displayName ?? replica.host ?? replica.instanceId, style: const TextStyle(fontSize: 13)),
                            subtitle: Text(
                              '${replica.replicaType.name} · ${replica.observedIp ?? replica.host ?? 'ip unknown'} · #${replica.replicaId}',
                              style: const TextStyle(fontSize: 11),
                            ),
                            trailing: StatusBadge(replica.status.name),
                            onTap: () {
                              setState(() => _selectedReplicaId = replica.replicaId);
                              _loadSamples(replica.replicaId);
                            },
                          );
                        },
                      ),
              ),
            ],
          ),
        ),
        second: PanelCard(
          child: selected == null
              ? const EmptyState(message: 'Select a replica to inspect its health, address, and runtime details.')
              : SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  selected.displayName ?? selected.host ?? selected.instanceId,
                                  style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 16),
                                ),
                                Text('${selected.replicaType.name} · runtime ${selected.runtimeId}', style: TextStyle(fontSize: 12, color: AppColors.textMuted)),
                              ],
                            ),
                          ),
                          StatusBadge(selected.status.name),
                        ],
                      ),
                      const SizedBox(height: 16),
                      _DetailGrid(selected: selected),
                      const SizedBox(height: 16),
                      Row(
                        children: [
                          const Text('Telemetry', style: TextStyle(fontWeight: FontWeight.w700)),
                          const Spacer(),
                          Text(
                            _samplesLoading ? 'loading…' : '${_samples.length} sample(s), last hour',
                            style: TextStyle(fontSize: 12, color: AppColors.textMuted),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      Wrap(
                        spacing: 10,
                        runSpacing: 10,
                        children: [
                          Sparkline(label: 'CPU', values: _samples.map((s) => s.cpuPercent).toList(), max: 100, unit: '%'),
                          Sparkline(label: 'Memory', values: _samples.map((s) => s.memPercent).toList(), max: 100, unit: '%', color: Color(0xFF7C5CFF)),
                          Sparkline(label: 'Process CPU', values: _samples.map((s) => s.processCpuPercent).toList(), color: Color(0xFF0EA5A5)),
                          Sparkline(label: 'Load (1m)', values: _samples.map((s) => s.loadOne ?? 0).toList(), color: Color(0xFFF59E0B)),
                          Sparkline(label: 'Net In', values: _samples.map((s) => s.netRxBytesPerSec).toList(), color: Color(0xFF22C55E), format: formatRate),
                          Sparkline(label: 'Net Out', values: _samples.map((s) => s.netTxBytesPerSec).toList(), color: Color(0xFFEF4444), format: formatRate),
                        ],
                      ),
                      const SizedBox(height: 16),
                      const Text('Attributes', style: TextStyle(fontWeight: FontWeight.w700)),
                      const SizedBox(height: 8),
                      SizedBox(
                        height: 240,
                        child: JsonEditor(value: pretty(selected.attributes), onChanged: (_) {}, readOnly: true),
                      ),
                    ],
                  ),
                ),
        ),
      ),
    );
  }
}

class _DetailGrid extends StatelessWidget {
  const _DetailGrid({required this.selected});

  final ReplicaRecord selected;

  @override
  Widget build(BuildContext context) {
    final fields = [
      ('Replica ID', selected.replicaId),
      ('Observed IP', selected.observedIp ?? '-'),
      ('Host', selected.host ?? '-'),
      ('Port', selected.port?.toString() ?? '-'),
      ('Base Path', selected.basePath ?? '/'),
      ('Instance ID', selected.instanceId),
      ('Version', selected.version ?? '-'),
      ('First Seen', formatDate(selected.firstSeenAt)),
      ('Last Heartbeat', formatDate(selected.lastHeartbeatAt)),
      ('Last Seen', formatDate(selected.lastSeenAt)),
      ('Offline At', formatDate(selected.offlineAt)),
    ];

    return LayoutBuilder(
      builder: (context, constraints) {
        final columns = constraints.maxWidth > 640 ? 2 : 1;
        return GridView.count(
          crossAxisCount: columns,
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          childAspectRatio: columns == 2 ? 4.5 : 5.5,
          mainAxisSpacing: 8,
          crossAxisSpacing: 12,
          children: [
            for (final field in fields)
              Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(field.$1.toUpperCase(), style: TextStyle(fontSize: 10, color: AppColors.textMuted, letterSpacing: 0.4)),
                  Text(field.$2, style: const TextStyle(fontSize: 12)),
                ],
              ),
          ],
        );
      },
    );
  }
}
