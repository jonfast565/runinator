import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/realtime/event_stream_client.dart';
import '../../core/services/app_service.dart';
import '../theme/app_theme.dart';

class ConnectionStrip extends ConsumerWidget {
  const ConnectionStrip({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final app = ref.watch(appProvider);
    final streamLabel = switch (app.eventStreamState) {
      EventStreamState.connected => 'Live',
      EventStreamState.connecting => 'Connecting',
      EventStreamState.fallback => 'Polling',
      EventStreamState.disconnected => 'Offline',
    };

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      color: AppColors.surfaceSubtle,
      child: Wrap(
        spacing: 16,
        runSpacing: 4,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Text('Service: ${app.serviceUrl ?? '—'}', style: const TextStyle(fontSize: 11, color: AppColors.textMuted)),
          Text('Stream: $streamLabel', style: const TextStyle(fontSize: 11, color: AppColors.textMuted)),
          Text(
            'Replicas: ${app.replicaCounts.workers}w / ${app.replicaCounts.wakers}k / ${app.replicaCounts.webservices}ws',
            style: const TextStyle(fontSize: 11, color: AppColors.textMuted),
          ),
          if (app.statusText.isNotEmpty) Text(app.statusText, style: const TextStyle(fontSize: 11, color: AppColors.successFg)),
          if (app.errorText.isNotEmpty) Text(app.errorText, style: const TextStyle(fontSize: 11, color: AppColors.dangerFg)),
        ],
      ),
    );
  }
}
