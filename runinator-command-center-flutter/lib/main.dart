import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'core/api/http_runtime.dart';
import 'core/platform/index.dart';
import 'ui/adapters/storage.dart';
import 'ui/adapters/web_platform_adapter.dart';
import 'ui/command_center_app.dart';
import 'ui/shared/code_editor.dart';

Future<void> bootstrap() async {
  WidgetsFlutterBinding.ensureInitialized();
  final prefs = await SharedPreferences.getInstance();
  await configureStorage(prefs);

  setWsOriginProvider(() {
    // web builds use same-origin ws when no override is set.
    return Uri.base.origin;
  });

  final storage = SharedPreferencesStorage(prefs);
  setPlatformAdapter(
    createWebPlatformAdapter(
      readStorage: storage.read,
      writeStorage: (key, value) => storage.write(key, value),
      removeStorage: storage.remove,
    ),
  );
  setTextEditorHostFactory(FlutterTextEditorHostFactory());
}

void main() async {
  await bootstrap();
  runApp(const ProviderScope(child: CommandCenterApp()));
}
