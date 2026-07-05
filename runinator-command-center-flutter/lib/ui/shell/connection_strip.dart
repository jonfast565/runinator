import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/realtime/event_stream_client.dart';
import '../../core/services/app_service.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';

class ConnectionStrip extends ConsumerWidget {
  const ConnectionStrip({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final app = ref.watch(appProvider);
    final (streamLabel, dotColor, pulse) = switch (app.eventStreamState) {
      EventStreamState.connected => ('Live', AppColors.accentPulse, true),
      EventStreamState.connecting => ('Connecting', AppColors.warningFg, false),
      EventStreamState.fallback => ('Polling', AppColors.warningFg, false),
      EventStreamState.disconnected => ('Offline', AppColors.dangerFg, false),
    };

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 7),
      color: AppColors.surfaceSubtle,
      child: Wrap(
        spacing: 18,
        runSpacing: 4,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              StatusDot(color: dotColor, pulse: pulse, size: 6),
              const SizedBox(width: 6),
              Text(streamLabel, style: TextStyle(fontSize: 11.5, color: AppColors.textMuted, fontWeight: FontWeight.w600)),
            ],
          ),
          Text(app.serviceUrl ?? '—', style: TextStyle(fontSize: 11.5, color: AppColors.textMuted)),
          Text(
            '${app.replicaCounts.workers}w · ${app.replicaCounts.wakers}k · ${app.replicaCounts.webservices}ws',
            style: TextStyle(fontSize: 11.5, color: AppColors.textMuted),
          ),
          if (app.statusText.isNotEmpty) Text(app.statusText, style: TextStyle(fontSize: 11.5, color: AppColors.successFg)),
          if (app.errorText.isNotEmpty) Text(app.errorText, style: TextStyle(fontSize: 11.5, color: AppColors.dangerFg)),
        ],
      ),
    );
  }
}
