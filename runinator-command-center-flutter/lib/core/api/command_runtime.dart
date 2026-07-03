// port of core/api/runtime.ts.
//
// dart has no reified generics worth preserving here, so `invoke` returns the
// decoded json (Object?) directly; typed parsing happens at the
// command_center_api.dart facade layer via each model's fromJson.

abstract class CommandRuntime {
  bool isTauri();

  Future<Object?> invoke(String name, [Map<String, Object?>? args]);

  String wsBaseUrl();

  String apiBaseUrl();
}

CommandRuntime? _activeRuntime;

void setCommandRuntime(CommandRuntime runtime) {
  _activeRuntime = runtime;
}

CommandRuntime? getCommandRuntimeOptional() => _activeRuntime;

CommandRuntime getCommandRuntime() {
  final runtime = _activeRuntime;
  if (runtime == null) {
    throw StateError('Command runtime has not been configured. Call setCommandRuntime() at bootstrap.');
  }

  return runtime;
}

bool isTauriRuntime() => getCommandRuntimeOptional()?.isTauri() ?? false;

Future<Object?> command(String name, [Map<String, Object?>? args]) =>
    getCommandRuntime().invoke(name, args);
