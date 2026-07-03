import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/replica.dart';
import '../../core/services/app_service.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';

class ReplicasView extends ConsumerWidget {
  const ReplicasView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final app = ref.watch(appProvider);
    final query = ref.read(appProvider.notifier).normalizedSearch;
    final replicas = app.replicas.where((replica) {
      if (query.isEmpty) return true;
      return [replica.displayName, replica.host, replica.instanceId, replica.replicaType.name, replica.status.name]
          .any((v) => displayValue(v).toLowerCase().contains(query));
    }).toList();

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        initialFirstFraction: 0.45,
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
              Text(
                'Live: ${app.replicaCounts.workers} workers, ${app.replicaCounts.wakers} wakers, ${app.replicaCounts.webservices} web services',
                style: const TextStyle(fontSize: 12, color: AppColors.textMuted),
              ),
              Expanded(
                child: CcDataTable(
                  columns: const ['Type', 'Status', 'Name', 'Host'],
                  rows: [
                    for (final replica in replicas)
                      [replica.replicaType.name, replica.status.name, replica.displayName ?? replica.instanceId, replica.host ?? ''],
                  ],
                  emptyMessage: 'No replicas discovered.',
                ),
              ),
            ],
          ),
        ),
        second: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Text('Replica telemetry', style: TextStyle(fontWeight: FontWeight.w700)),
              const SizedBox(height: 8),
              Text('Workers: ${app.replicaCounts.workers}', style: const TextStyle(fontSize: 12)),
              Text('Wakers: ${app.replicaCounts.wakers}', style: const TextStyle(fontSize: 12)),
              Text('Web services: ${app.replicaCounts.webservices}', style: const TextStyle(fontSize: 12)),
              const SizedBox(height: 12),
              const Text('Local worker controls are available in the desktop client.', style: TextStyle(color: AppColors.textMuted, fontSize: 12)),
            ],
          ),
        ),
      ),
    );
  }
}
