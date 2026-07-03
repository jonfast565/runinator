import '../../core/api/command_center_api.dart';
import '../../core/api/command_runtime.dart';
import '../../core/api/http_runtime.dart';
import '../../core/domain/models/index.dart';
import '../../core/platform/types.dart';
import 'browser_runtime.dart';
import 'artifact_transport.dart';

class _PrefsAuthStorage implements AuthStorage {
  _PrefsAuthStorage(this._read, this._write, this._remove);

  final String? Function(String key) _read;
  final void Function(String key, String value) _write;
  final void Function(String key) _remove;

  @override
  String? get(String key) => _read(key);

  @override
  void set(String key, String value) => _write(key, value);

  @override
  void remove(String key) => _remove(key);
}

class _FlutterDialogs implements PlatformDialogs {
  @override
  bool confirm(String message) => true;

  @override
  String? prompt(String message) => null;
}

class _WebServiceDiscovery implements ServiceDiscovery {
  @override
  bool isDesktop() => false;

  @override
  String webServiceUrl() => wsBaseUrl();

  @override
  Future<ServiceStatusSnapshot> getInitialStatus() async =>
      ServiceStatusSnapshot(serviceUrl: wsBaseUrl().isEmpty ? null : wsBaseUrl());

  @override
  Future<void> startDiscovery() async {}

  @override
  Future<Unsubscribe> listenServiceUrlChanged(ServiceUrlChangedHandler handler) async => () {};

  @override
  Future<Unsubscribe> listenDiscoveryError(DiscoveryErrorHandler handler) async => () {};
}

class _WebArtifactTransport extends WebArtifactTransport {}

PlatformAdapter createWebPlatformAdapter({
  required String? Function(String key) readStorage,
  required void Function(String key, String value) writeStorage,
  required void Function(String key) removeStorage,
}) {
  return _WebPlatformAdapter(
    authStorage: _PrefsAuthStorage(readStorage, writeStorage, removeStorage),
  );
}

class _WebPlatformAdapter implements PlatformAdapter {
  _WebPlatformAdapter({required this.authStorage});

  @override
  final AuthStorage authStorage;

  @override
  CommandRuntime get runtime => browserCommandRuntime;

  @override
  PlatformDialogs get dialogs => _FlutterDialogs();

  @override
  ArtifactTransport get artifacts => _WebArtifactTransport();

  @override
  ServiceDiscovery get serviceDiscovery => _WebServiceDiscovery();

  @override
  FilePicker? get filePicker => _WebFilePicker();
}

class _WebFilePicker implements FilePicker {
  final _transport = _WebArtifactTransport();

  @override
  Future<Object?> pickFile() => _transport.pickFile();
}
