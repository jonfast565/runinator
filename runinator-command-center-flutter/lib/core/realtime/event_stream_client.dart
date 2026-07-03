// port of core/realtime/event-stream-client.ts.
//
// dart's web_socket_channel exposes connection success/failure through the
// `ready` future rather than an onopen callback; that future drives the
// "connecting -> connected" transition here in place of the ts source's
// WebSocket.onopen handler. everything else (fallback polling, reconnect
// delay, connect timeout race, connection-id fencing against stale
// callbacks) is ported verbatim.

import 'dart:async';
import 'dart:convert';

import 'package:web_socket_channel/web_socket_channel.dart';

import 'event_router.dart';
import 'websocket_url.dart';

const Duration _reconnectDelay = Duration(milliseconds: 3000);
const Duration _fallbackInterval = Duration(milliseconds: 30000);
const Duration _connectTimeout = Duration(milliseconds: 5000);

enum EventStreamState { disconnected, connecting, connected, fallback }

class EventStreamClientOptions {
  const EventStreamClientOptions({
    required this.getServiceUrl,
    required this.getServiceKnown,
    required this.onStateChange,
    required this.onFallbackTick,
    required this.router,
  });

  final String? Function() getServiceUrl;
  final bool Function() getServiceKnown;
  final void Function(EventStreamState state) onStateChange;
  final void Function() onFallbackTick;
  final EventStreamRouter router;
}

class EventStreamClient {
  EventStreamClient(this._options);

  final EventStreamClientOptions _options;

  WebSocketChannel? _channel;
  StreamSubscription<Object?>? _subscription;
  Timer? _fallbackTimer;
  Timer? _reconnectTimer;
  Timer? _connectTimer;
  int _connectionId = 0;

  void connect() {
    _clearReconnectTimer();
    _clearConnectTimer();
    final serviceUrl = _options.getServiceUrl();

    if (serviceUrl == null) {
      _startFallback();
      return;
    }

    final currentConnection = ++_connectionId;
    _options.onStateChange(EventStreamState.connecting);
    final url = buildWebSocketUrl(serviceUrl, '/ws/events');

    final channel = WebSocketChannel.connect(Uri.parse(url));
    _channel = channel;

    _connectTimer = Timer(_connectTimeout, () {
      if (currentConnection != _connectionId) {
        return;
      }

      _channel?.sink.close();
      _startFallback();
    });

    channel.ready.then((_) {
      if (currentConnection != _connectionId) {
        return;
      }

      _clearConnectTimer();
      _options.onStateChange(EventStreamState.connected);
      _stopFallback();
    }).catchError((Object _) {
      // connection failed to establish; the stream's onDone below drives fallback/reconnect.
    });

    _subscription = channel.stream.listen(
      (data) {
        if (currentConnection != _connectionId) {
          return;
        }

        try {
          _options.router.route(jsonDecode(data as String) as ServerEvent);
        } catch (_) {
          // ignore malformed payloads
        }
      },
      onDone: () {
        if (currentConnection != _connectionId) {
          return;
        }

        _clearConnectTimer();
        _channel = null;
        _startFallback();

        if (_options.getServiceKnown()) {
          _reconnectTimer = Timer(_reconnectDelay, connect);
        }
      },
      onError: (Object _) {
        if (currentConnection != _connectionId) {
          return;
        }

        _clearConnectTimer();
        _channel?.sink.close();
      },
      cancelOnError: true,
    );
  }

  void disconnect() {
    _connectionId += 1;
    _clearReconnectTimer();
    _clearConnectTimer();
    _subscription?.cancel();
    _channel?.sink.close();
    _channel = null;
    _stopFallback();
    _options.onStateChange(EventStreamState.disconnected);
  }

  void _startFallback() {
    if (_fallbackTimer != null) {
      return;
    }

    _options.onStateChange(EventStreamState.fallback);
    _fallbackTimer = Timer.periodic(_fallbackInterval, (_) => _options.onFallbackTick());
  }

  void _stopFallback() {
    _fallbackTimer?.cancel();
    _fallbackTimer = null;
  }

  void _clearReconnectTimer() {
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
  }

  void _clearConnectTimer() {
    _connectTimer?.cancel();
    _connectTimer = null;
  }
}
