// port of core/services/notifications.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import 'app_service.dart';

part 'notifications_service.g.dart';

class NotificationsState {
  const NotificationsState({required this.notifications, required this.unreadOnly});

  final List<Notification> notifications;
  final bool unreadOnly;

  NotificationsState copyWith({List<Notification>? notifications, bool? unreadOnly}) => NotificationsState(
        notifications: notifications ?? this.notifications,
        unreadOnly: unreadOnly ?? this.unreadOnly,
      );
}

@riverpod
class NotificationsNotifier extends _$NotificationsNotifier {
  @override
  NotificationsState build() => const NotificationsState(notifications: [], unreadOnly: false);

  int unreadCount() => state.notifications.where((n) => n.readAt == null).length;

  void setUnreadOnly(bool value) {
    state = state.copyWith(unreadOnly: value);
  }

  Future<void> refreshNotifications() async {
    final app = ref.read(appProvider.notifier);
    List<Notification> notifications;
    try {
      notifications = await app.runOperation(
        'Loading notifications',
        () => api.fetchNotifications(unreadOnly: state.unreadOnly),
      );
    } catch (_) {
      notifications = [];
    }
    state = state.copyWith(notifications: notifications);
  }

  void clearNotifications() {
    state = state.copyWith(notifications: const []);
  }

  Future<void> markRead(String id) async {
    final app = ref.read(appProvider.notifier);
    try {
      await app.runOperation('Marking notification read', () => api.markNotificationRead(id));
    } catch (error) {
      app.setError(error.toString());
    }
    await refreshNotifications();
  }

  Future<void> markAllRead() async {
    final app = ref.read(appProvider.notifier);
    try {
      await app.runOperation('Marking all notifications read', api.markAllNotificationsRead);
    } catch (error) {
      app.setError(error.toString());
    }
    await refreshNotifications();
  }

  Future<void> remove(String id) async {
    final app = ref.read(appProvider.notifier);
    try {
      await app.runOperation('Deleting notification', () => api.deleteNotification(id));
    } catch (error) {
      app.setError(error.toString());
    }
    state = state.copyWith(notifications: state.notifications.where((n) => n.id != id).toList());
    await refreshNotifications();
  }

  Future<void> removeAllRead() async {
    final app = ref.read(appProvider.notifier);
    final readIds = state.notifications.where((n) => n.readAt != null).map((n) => n.id).toList();

    if (readIds.isEmpty) {
      return;
    }

    try {
      await app.runOperation('Deleting read notifications', () async {
        for (final id in readIds) {
          await api.deleteNotification(id);
        }
      });
    } catch (error) {
      app.setError(error.toString());
    }
    await refreshNotifications();
  }
}
