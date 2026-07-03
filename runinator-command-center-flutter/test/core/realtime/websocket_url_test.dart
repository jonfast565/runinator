import 'package:runinator_command_center_flutter/core/api/http_runtime.dart' show setHttpAuthToken;
import 'package:runinator_command_center_flutter/core/realtime/websocket_url.dart';
import 'package:test/test.dart';

void main() {
  group('websocket url utils', () {
    setUp(() {
      setHttpAuthToken(null);
    });

    test('builds a websocket url from a trailing slash service url', () {
      expect(
        buildWebSocketUrl('http://127.0.0.1:8080/', '/ws/events'),
        'ws://127.0.0.1:8080/ws/events',
      );
    });

    test('preserves a configured base path', () {
      expect(
        buildWebSocketUrl('http://127.0.0.1:8080/api/', '/ws/events'),
        'ws://127.0.0.1:8080/api/ws/events',
      );
    });

    test('uses secure websockets for https service urls', () {
      expect(
        buildWebSocketUrl('https://example.test/api/', '/ws/events'),
        'wss://example.test/api/ws/events',
      );
    });

    test('appends the access token as a query param for browser websocket auth', () {
      setHttpAuthToken('token-123');

      expect(
        buildWebSocketUrl('https://example.test/api/', '/ws/events'),
        'wss://example.test/api/ws/events?token=token-123',
      );
    });
  });
}
