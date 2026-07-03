// port of ui/composables/useWorkflowNodeRunLogStream.ts.

import 'dart:async';
import 'dart:convert';

import 'package:web_socket_channel/web_socket_channel.dart';

import '../domain/models/index.dart';
import 'websocket_url.dart';

const _reconnectDelay = Duration(seconds: 3);

class WorkflowNodeLogStreamClient {
  WorkflowNodeLogStreamClient({
    required this.getServiceUrl,
    required this.getServiceKnown,
  });

  final String? Function() getServiceUrl;
  final bool Function() getServiceKnown;

  WebSocketChannel? _channel;
  Timer? _reconnectTimer;
  var _connectionId = 0;
  String? _activeNodeRunId;

  final chunks = <RunChunk>[];
  var lastChunkAt = 0;

  void connect(String nodeRunId) {
    disconnect(clearChunks: true);
    _activeNodeRunId = nodeRunId;

    final serviceUrl = getServiceUrl();
    if (nodeRunId.isEmpty || serviceUrl == null || serviceUrl.isEmpty) {
      return;
    }

    final currentConnection = ++_connectionId;
    final channel = WebSocketChannel.connect(
      Uri.parse(buildWebSocketUrl(serviceUrl, '/ws/workflow-node-runs/$nodeRunId/stream')),
    );
    _channel = channel;

    channel.stream.listen(
      (data) {
        if (currentConnection != _connectionId) return;

        try {
          final decoded = jsonDecode(data as String) as Map<String, Object?>;
          chunks.add(RunChunk.fromJson(decoded));
          lastChunkAt = DateTime.now().millisecondsSinceEpoch;
        } catch (_) {
          // ignore malformed stream payloads.
        }
      },
      onError: (_) {
        if (currentConnection != _connectionId) return;
        channel.sink.close();
      },
      onDone: () {
        if (currentConnection != _connectionId) return;

        _channel = null;
        final id = _activeNodeRunId;

        if (id == nodeRunId && getServiceKnown()) {
          _reconnectTimer = Timer(_reconnectDelay, () => connect(nodeRunId));
        }
      },
      cancelOnError: true,
    );
  }

  void disconnect({bool clearChunks = false}) {
    _connectionId += 1;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    _channel?.sink.close();
    _channel = null;

    if (clearChunks) {
      chunks.clear();
      lastChunkAt = 0;
      _activeNodeRunId = null;
    }
  }

  void dispose() => disconnect(clearChunks: true);
}
