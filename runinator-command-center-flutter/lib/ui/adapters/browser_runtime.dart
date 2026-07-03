import '../../core/api/command_runtime.dart';
import '../../core/api/http_runtime.dart' as http_runtime;

class BrowserCommandRuntime implements CommandRuntime {
  @override
  bool isTauri() => false;

  @override
  Future<Object?> invoke(String name, [Map<String, Object?>? args]) => http_runtime.invokeViaHttp(name, args);

  @override
  String wsBaseUrl() => http_runtime.wsBaseUrl();

  @override
  String apiBaseUrl() => http_runtime.apiBaseUrl();
}

final CommandRuntime browserCommandRuntime = BrowserCommandRuntime();
