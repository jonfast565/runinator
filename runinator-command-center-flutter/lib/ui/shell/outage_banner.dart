import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/services/app_service.dart';
import '../theme/app_theme.dart';

class OutageBanner extends ConsumerWidget {
  const OutageBanner({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final app = ref.watch(appProvider);
    if (app.backendReachable || app.outageDismissed || app.serviceUrl == null) {
      return const SizedBox.shrink();
    }

    return Material(
      color: AppColors.dangerBg,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        child: Row(
          children: [
            const Expanded(
              child: Text(
                'Cannot reach the Runinator backend. Some actions may fail until connectivity is restored.',
                style: TextStyle(color: AppColors.dangerFg, fontSize: 12),
              ),
            ),
            TextButton(
              onPressed: () => ref.read(appProvider.notifier).dismissOutageBanner(),
              child: const Text('Dismiss'),
            ),
          ],
        ),
      ),
    );
  }
}
