// port of core/utils/websocket.ts.

import '../api/http_runtime.dart' show httpAuthToken;

String buildWebSocketUrl(String serviceUrl, String routePath) {
  final parsed = Uri.parse(serviceUrl);

  String scheme;
  if (parsed.scheme == 'http') {
    scheme = 'ws';
  } else if (parsed.scheme == 'https') {
    scheme = 'wss';
  } else if (parsed.scheme == 'ws' || parsed.scheme == 'wss') {
    scheme = parsed.scheme;
  } else {
    throw ArgumentError('Unsupported WebSocket base protocol: ${parsed.scheme}:');
  }

  final basePath = parsed.path.replaceAll(RegExp(r'/+$'), '');
  final route = routePath.replaceFirst(RegExp(r'^/+'), '');
  final path = '$basePath/$route'.replaceAll(RegExp(r'/{2,}'), '/');

  // browsers can't set headers on a WebSocket upgrade, so the access token rides as a query param.
  final token = httpAuthToken();

  return Uri(
    scheme: scheme,
    userInfo: parsed.userInfo.isEmpty ? null : parsed.userInfo,
    host: parsed.host,
    port: parsed.hasPort ? parsed.port : null,
    path: path,
    queryParameters: token != null ? {'token': token} : null,
  ).toString();
}
