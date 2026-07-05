// port of ui/composables/useWorkflowRunStream.ts.

import 'dart:async';
import 'dart:convert';

import 'package:web_socket_channel/web_socket_channel.dart';

import '../domain/models/index.dart';
import '../utils/status.dart';
import 'reconnect_backoff.dart';
import 'websocket_url.dart';

class _RunStreamHandle {
  WebSocketChannel? socket;
  Timer? reconnectTimer;
  var connectionId = 0;
  var terminal = false;
  var disposed = false;
  final backoff = ReconnectBackoff();
}

typedef WorkflowRunDetailHandler = void Function(WorkflowRunDetail detail);

class WorkflowRunStreamClient {
  WorkflowRunStreamClient({
    required this.getServiceUrl,
    required this.getServiceKnown,
    required this.getOpenRunIds,
    required this.onDetail,
  });

  final String? Function() getServiceUrl;
  final bool Function() getServiceKnown;
  final List<String> Function() getOpenRunIds;
  final WorkflowRunDetailHandler onDetail;

  final _sockets = <String, _RunStreamHandle>{};

  void sync() {
    final ids = getOpenRunIds().where((id) => id.isNotEmpty).toSet();

    for (final id in [..._sockets.keys]) {
      if (!ids.contains(id)) {
        _disposeHandle(id);
      }
    }

    for (final id in ids) {
      if (!_sockets.containsKey(id)) {
        _connect(id);
      }
    }
  }

  void reconnectAll() {
    for (final id in [..._sockets.keys]) {
      _disposeHandle(id);
    }
    sync();
  }

  void dispose() {
    for (final id in [..._sockets.keys]) {
      _disposeHandle(id);
    }
  }

  void _disposeHandle(String runId) {
    final handle = _sockets.remove(runId);
    if (handle == null) return;

    handle.disposed = true;
    handle.connectionId += 1;
    handle.reconnectTimer?.cancel();
    handle.reconnectTimer = null;
    handle.socket?.sink.close();
    handle.socket = null;
  }

  _RunStreamHandle _ensureHandle(String runId) {
    return _sockets.putIfAbsent(runId, () => _RunStreamHandle());
  }

  void _connect(String runId) {
    final serviceUrl = getServiceUrl();
    if (runId.isEmpty || serviceUrl == null || serviceUrl.isEmpty) {
      return;
    }

    final handle = _ensureHandle(runId);
    handle.reconnectTimer?.cancel();
    handle.reconnectTimer = null;

    final myConnectionId = ++handle.connectionId;
    final channel = WebSocketChannel.connect(
      Uri.parse(buildWebSocketUrl(serviceUrl, '/ws/workflow-runs/$runId')),
    );
    handle.socket = channel;

    channel.ready.then((_) {
      if (handle.disposed || handle.connectionId != myConnectionId) return;
      handle.backoff.reset();
    }).catchError((Object _) {
      // connection failed to establish; onDone below drives the reconnect.
    });

    channel.stream.listen(
      (data) {
        if (handle.disposed || handle.connectionId != myConnectionId) return;

        try {
          final decoded = jsonDecode(data as String) as Map<String, Object?>;
          final detail = WorkflowRunDetail.fromJson(decoded);
          onDetail(detail);

          if (isTerminalWorkflowRunStatus(detail.run.status)) {
            handle.terminal = true;
          }
        } catch (_) {
          // ignore malformed stream payloads.
        }
      },
      onError: (_) {
        if (handle.disposed || handle.connectionId != myConnectionId) return;
        channel.sink.close();
      },
      onDone: () {
        if (handle.disposed || handle.connectionId != myConnectionId) return;

        handle.socket = null;

        if (handle.terminal) return;

        if (getOpenRunIds().contains(runId) && getServiceKnown()) {
          handle.reconnectTimer = Timer(handle.backoff.next(), () => _connect(runId));
        }
      },
      cancelOnError: true,
    );
  }
}
