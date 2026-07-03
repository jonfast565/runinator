// port of core/platform/index.ts.

import '../api/command_runtime.dart';
import 'text_editor.dart';
import 'types.dart';

export 'text_editor.dart';
export 'types.dart';

PlatformAdapter? _activePlatform;
TextEditorHostFactory? _activeTextEditorFactory;

void setPlatformAdapter(PlatformAdapter adapter) {
  _activePlatform = adapter;
  setCommandRuntime(adapter.runtime);
}

PlatformAdapter getPlatformAdapter() {
  final platform = _activePlatform;

  if (platform == null) {
    throw StateError('Platform adapter has not been configured. Configure it at bootstrap.');
  }

  return platform;
}

PlatformAdapter? getPlatformAdapterOptional() => _activePlatform;

void setTextEditorHostFactory(TextEditorHostFactory factory) {
  _activeTextEditorFactory = factory;
}

TextEditorHostFactory getTextEditorHostFactory() {
  final factory = _activeTextEditorFactory;

  if (factory == null) {
    throw StateError('Text editor host factory has not been configured.');
  }

  return factory;
}
