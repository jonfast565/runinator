// port of core/services/app.ts.
//
// the ts source's createStore<T>()/subscribe pattern (event-bus.ts) is superseded
// entirely by riverpod's Notifier: `state = ...` assignment is both the store
// write and the notify-listeners step. the manual `dispose()` method is likewise
// replaced by `ref.onDispose(...)` registered in build(), since riverpod already
// owns the notifier's lifecycle.
//
// the ts source's local `EventStreamState` type alias is identical to
// core/realtime/event_stream_client.dart's enum of the same name, so that type is
// reused here rather than redeclared.

import 'dart:async';

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' show fetchReplicas;
import '../domain/models/index.dart';
import '../navigation/app_tab.dart';
import '../navigation/nav_config.dart';
import '../realtime/event_stream_client.dart' show EventStreamState;

part 'app_service.g.dart';

enum ToastKind { info, loading, success, error }

class Toast {
  const Toast({required this.id, required this.kind, required this.text});

  final int id;
  final ToastKind kind;
  final String text;
}

const Map<ToastKind, int?> _toastTimeoutsMs = {
  ToastKind.info: 5000,
  ToastKind.loading: null,
  ToastKind.success: 5000,
  ToastKind.error: 8000,
};

const int _maxToasts = 4;

const Object _unset = Object();

class AppState {
  const AppState({
    required this.activeTab,
    required this.sidebarCollapsed,
    required this.mobileNavOpen,
    this.serviceUrl,
    required this.backendReachable,
    required this.outageDismissed,
    required this.initialLoading,
    required this.loading,
    required this.opLabel,
    required this.statusText,
    required this.errorText,
    required this.searchQuery,
    this.lastRefreshAt,
    required this.eventStreamState,
    required this.replicaCounts,
    required this.replicas,
    required this.toasts,
  });

  final AppTab activeTab;
  final bool sidebarCollapsed;
  final bool mobileNavOpen;
  final String? serviceUrl;
  final bool backendReachable;
  final bool outageDismissed;
  final bool initialLoading;
  final bool loading;
  final String opLabel;
  final String statusText;
  final String errorText;
  final String searchQuery;
  final DateTime? lastRefreshAt;
  final EventStreamState eventStreamState;
  final ReplicaCounts replicaCounts;
  final List<ReplicaRecord> replicas;
  final List<Toast> toasts;

  AppState copyWith({
    AppTab? activeTab,
    bool? sidebarCollapsed,
    bool? mobileNavOpen,
    Object? serviceUrl = _unset,
    bool? backendReachable,
    bool? outageDismissed,
    bool? initialLoading,
    bool? loading,
    String? opLabel,
    String? statusText,
    String? errorText,
    String? searchQuery,
    Object? lastRefreshAt = _unset,
    EventStreamState? eventStreamState,
    ReplicaCounts? replicaCounts,
    List<ReplicaRecord>? replicas,
    List<Toast>? toasts,
  }) =>
      AppState(
        activeTab: activeTab ?? this.activeTab,
        sidebarCollapsed: sidebarCollapsed ?? this.sidebarCollapsed,
        mobileNavOpen: mobileNavOpen ?? this.mobileNavOpen,
        serviceUrl: identical(serviceUrl, _unset) ? this.serviceUrl : serviceUrl as String?,
        backendReachable: backendReachable ?? this.backendReachable,
        outageDismissed: outageDismissed ?? this.outageDismissed,
        initialLoading: initialLoading ?? this.initialLoading,
        loading: loading ?? this.loading,
        opLabel: opLabel ?? this.opLabel,
        statusText: statusText ?? this.statusText,
        errorText: errorText ?? this.errorText,
        searchQuery: searchQuery ?? this.searchQuery,
        lastRefreshAt: identical(lastRefreshAt, _unset) ? this.lastRefreshAt : lastRefreshAt as DateTime?,
        eventStreamState: eventStreamState ?? this.eventStreamState,
        replicaCounts: replicaCounts ?? this.replicaCounts,
        replicas: replicas ?? this.replicas,
        toasts: toasts ?? this.toasts,
      );
}

AppState _initialAppState() => AppState(
      activeTab: readStoredDefaultTab(),
      sidebarCollapsed: readSidebarCollapsed(),
      mobileNavOpen: false,
      serviceUrl: null,
      backendReachable: false,
      outageDismissed: false,
      initialLoading: true,
      loading: false,
      opLabel: '',
      statusText: '',
      errorText: '',
      searchQuery: '',
      lastRefreshAt: null,
      eventStreamState: EventStreamState.disconnected,
      replicaCounts: const ReplicaCounts(workers: 0, wakers: 0, webservices: 0),
      replicas: const [],
      toasts: const [],
    );

/// mirrors the ts source's fetch()-specific TypeError check with a DioException
/// connection-error check, plus the same message-substring heuristics.
bool isNetworkError(Object error) {
  final typeName = error.runtimeType.toString();
  if (typeName.contains('DioException')) {
    final str = error.toString().toLowerCase();
    if (str.contains('connectionerror') || str.contains('connection timeout') || str.contains('connecting timeout')) {
      return true;
    }
  }

  final message = error.toString().toLowerCase();
  return message.contains('failed to fetch') ||
      message.contains('load failed') ||
      message.contains('networkerror') ||
      message.contains('network request failed');
}

@riverpod
class AppNotifier extends _$AppNotifier {
  Timer? _statusTimer;
  int _toastSeq = 0;
  final Map<int, Timer> _toastTimers = {};

  @override
  AppState build() {
    ref.onDispose(() {
      _statusTimer?.cancel();
      for (final timer in _toastTimers.values) {
        timer.cancel();
      }
      _toastTimers.clear();
    });

    return _initialAppState();
  }

  void resetForTests() {
    _statusTimer?.cancel();
    for (final timer in _toastTimers.values) {
      timer.cancel();
    }
    _toastTimers.clear();
    state = _initialAppState();
  }

  String get normalizedSearch => state.searchQuery.trim().toLowerCase();

  void setActiveTab(AppTab tab) {
    state = state.copyWith(activeTab: tab, mobileNavOpen: false);
  }

  void openMobileNav() {
    state = state.copyWith(mobileNavOpen: true);
  }

  void closeMobileNav() {
    state = state.copyWith(mobileNavOpen: false);
  }

  void toggleMobileNav() {
    state = state.copyWith(mobileNavOpen: !state.mobileNavOpen);
  }

  void toggleSidebar() {
    final sidebarCollapsed = !state.sidebarCollapsed;
    setNavStorageWriter?.call('command-center.sidebar.collapsed', sidebarCollapsed.toString());
    state = state.copyWith(sidebarCollapsed: sidebarCollapsed);
  }

  void setStatus(String text) {
    state = state.copyWith(statusText: text, errorText: '', lastRefreshAt: DateTime.now());
    _statusTimer?.cancel();
    _statusTimer = Timer(const Duration(milliseconds: 5000), () {
      state = state.copyWith(statusText: '');
    });
    pushToast(ToastKind.success, text);
  }

  void setError(String text) {
    state = state.copyWith(errorText: text, statusText: '', initialLoading: false);
    pushToast(ToastKind.error, text);
  }

  int pushToast(ToastKind kind, String text) {
    final id = ++_toastSeq;
    final nextToasts = [...state.toasts, Toast(id: id, kind: kind, text: text)];
    state = state.copyWith(
      toasts: nextToasts.length > _maxToasts ? nextToasts.sublist(nextToasts.length - _maxToasts) : nextToasts,
    );
    final timeout = _toastTimeoutsMs[kind];

    if (timeout != null) {
      _toastTimers[id] = Timer(Duration(milliseconds: timeout), () => dismissToast(id));
    }

    return id;
  }

  void dismissToast(int id) {
    state = state.copyWith(toasts: state.toasts.where((toast) => toast.id != id).toList());
    _toastTimers.remove(id)?.cancel();
  }

  void clearToasts() {
    for (final timer in _toastTimers.values) {
      timer.cancel();
    }
    _toastTimers.clear();
    state = state.copyWith(toasts: const []);
  }

  void markBackendReachable() {
    state = state.copyWith(backendReachable: true, outageDismissed: false);
  }

  void markBackendUnreachable() {
    state = state.copyWith(backendReachable: false);
  }

  void dismissOutageBanner() {
    state = state.copyWith(outageDismissed: true);
  }

  void setServiceUrl(String? url) {
    state = state.copyWith(
      serviceUrl: url,
      backendReachable: url != null && url.isNotEmpty,
      errorText: (url != null && url.isNotEmpty) ? '' : state.errorText,
      eventStreamState: (url != null && url.isNotEmpty) ? state.eventStreamState : EventStreamState.disconnected,
      replicas: (url != null && url.isNotEmpty) ? state.replicas : const [],
      replicaCounts:
          (url != null && url.isNotEmpty) ? state.replicaCounts : const ReplicaCounts(workers: 0, wakers: 0, webservices: 0),
    );
  }

  void setEventStreamState(EventStreamState next) {
    state = state.copyWith(eventStreamState: next);
  }

  void setReplicaState(List<ReplicaRecord> nextReplicas, [ReplicaCounts? nextCounts]) {
    state = state.copyWith(
      replicas: [...nextReplicas],
      replicaCounts: nextCounts ??
          ReplicaCounts(
            workers: nextReplicas.where((r) => r.replicaType == ReplicaKind.worker && r.status == ReplicaStatus.live).length,
            wakers: nextReplicas.where((r) => r.replicaType == ReplicaKind.waker && r.status == ReplicaStatus.live).length,
            webservices:
                nextReplicas.where((r) => r.replicaType == ReplicaKind.webservice && r.status == ReplicaStatus.live).length,
          ),
    );
  }

  void clearReplicaState() {
    state = state.copyWith(replicas: const [], replicaCounts: const ReplicaCounts(workers: 0, wakers: 0, webservices: 0));
  }

  void setInitialLoading(bool value) {
    state = state.copyWith(initialLoading: value);
  }

  void setSearchQuery(String query) {
    state = state.copyWith(searchQuery: query);
  }

  Future<void> refreshReplicas() async {
    final response = await fetchReplicas();
    final nextReplicas = [...response.replicas]
      ..sort((left, right) {
        final typeOrder = _replicaKindOrder(left.replicaType) - _replicaKindOrder(right.replicaType);

        if (typeOrder != 0) {
          return typeOrder;
        }

        final statusOrder = _replicaStatusOrder(left.status) - _replicaStatusOrder(right.status);

        if (statusOrder != 0) {
          return statusOrder;
        }

        return _replicaLabel(left).compareTo(_replicaLabel(right));
      });
    state = state.copyWith(replicas: nextReplicas, replicaCounts: response.counts);
  }

  Future<T> runOperation<T>(String label, Future<T> Function() operation) async {
    state = state.copyWith(loading: true, opLabel: label, errorText: '');
    final toastId = pushToast(ToastKind.loading, '$label...');

    try {
      final result = await operation();
      state = state.copyWith(backendReachable: true, outageDismissed: false);
      return result;
    } catch (error) {
      if (isNetworkError(error)) {
        state = state.copyWith(backendReachable: false);
      }

      final message = error.toString();
      state = state.copyWith(errorText: message, statusText: '', initialLoading: false);
      pushToast(ToastKind.error, message);
      rethrow;
    } finally {
      state = state.copyWith(loading: false, opLabel: '');
      dismissToast(toastId);
    }
  }
}

/// injectable hook mirroring nav_config.dart's setNavStorageReader; a concrete web
/// platform adapter (future UI pass) can persist the sidebar-collapsed flag.
void Function(String key, String value)? setNavStorageWriter;

int _replicaKindOrder(ReplicaKind kind) {
  switch (kind) {
    case ReplicaKind.webservice:
      return 0;
    case ReplicaKind.worker:
      return 1;
    case ReplicaKind.waker:
      return 2;
    default:
      return 3;
  }
}

int _replicaStatusOrder(ReplicaStatus status) {
  switch (status) {
    case ReplicaStatus.live:
      return 0;
    case ReplicaStatus.stale:
      return 1;
    case ReplicaStatus.offline:
      return 2;
  }
}

String _replicaLabel(ReplicaRecord replica) => replica.displayName ?? replica.host ?? replica.instanceId;
