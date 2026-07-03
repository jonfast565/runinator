import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/app_service.dart';
import '../../core/services/notifications_service.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/split_pane.dart';

class NotificationsView extends ConsumerWidget {
  const NotificationsView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(notificationsProvider);
    final notifier = ref.read(notificationsProvider.notifier);
    final app = ref.watch(appProvider);
    final query = ref.read(appProvider.notifier).normalizedSearch;
    final rows = state.notifications.where((item) {
      if (state.unreadOnly && item.readAt != null) return false;
      if (query.isEmpty) return true;
      return [item.title, item.body, item.id].any((v) => displayValue(v).toLowerCase().contains(query));
    }).toList();

    return Padding(
      padding: const EdgeInsets.all(12),
      child: PanelCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PanelToolbar(
              title: 'Notifications',
              actions: [
                CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.refreshNotifications()),
                CcButton(icon: IconName.check, label: 'Mark all read', dense: true, onPressed: () => notifier.markAllRead()),
                CcButton(icon: IconName.trash, label: 'Delete read', dense: true, onPressed: () => notifier.removeAllRead()),
              ],
            ),
            Expanded(
              child: CcDataTable(
                columns: const ['Read', 'Title', 'Created'],
                rows: [
                  for (final item in rows)
                    [item.readAt != null ? 'yes' : 'no', item.title, displayValue(item.createdAt)],
                ],
                emptyMessage: 'No notifications.',
              ),
            ),
          ],
        ),
      ),
    );
  }
}
