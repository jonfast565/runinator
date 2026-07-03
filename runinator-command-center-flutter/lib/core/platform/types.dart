// port of core/platform/types.ts.
//
// `File`/`HTMLElement` are browser-specific ts types with no Flutter-independent
// equivalent; both are represented as opaque `Object` here. The future UI pass's
// concrete web platform adapter narrows them to whatever `package:web` (or a
// Flutter file-picker plugin) actually returns/expects.

import '../api/command_center_api.dart' show ArtifactUploadRequest;
import '../api/command_runtime.dart';
import '../domain/models/index.dart';

abstract class AuthStorage {
  String? get(String key);

  void set(String key, String value);

  void remove(String key);
}

abstract class FilePicker {
  /// returns an opaque platform file handle, or null if the user canceled.
  Future<Object?> pickFile();
}

class ArtifactDownloadResult {
  const ArtifactDownloadResult({required this.savedTo});

  final String? savedTo;
}

abstract class ArtifactTransport {
  bool isDesktop();

  /// returns an opaque platform file handle, or null if the user canceled.
  Future<Object?> pickFile();

  Future<RunArtifact> uploadFromPath(ArtifactUploadRequest request);

  Future<RunArtifact> uploadFromBrowser(ArtifactUploadRequest request, Object file);

  Future<void> downloadInBrowser(String artifactId, String name);

  Future<ArtifactDownloadResult> downloadToPath(String artifactId, String name);
}

abstract class PlatformDialogs {
  bool confirm(String message);

  String? prompt(String message);
}

class ServiceStatusSnapshot {
  const ServiceStatusSnapshot({required this.serviceUrl});

  final String? serviceUrl;
}

typedef ServiceUrlChangedHandler = void Function(String? url);
typedef DiscoveryErrorHandler = void Function(String message);
typedef Unsubscribe = void Function();

abstract class ServiceDiscovery {
  bool isDesktop();

  String webServiceUrl();

  Future<ServiceStatusSnapshot> getInitialStatus();

  Future<void> startDiscovery();

  Future<Unsubscribe> listenServiceUrlChanged(ServiceUrlChangedHandler handler);

  Future<Unsubscribe> listenDiscoveryError(DiscoveryErrorHandler handler);
}

abstract class PlatformAdapter {
  CommandRuntime get runtime;

  AuthStorage get authStorage;

  PlatformDialogs get dialogs;

  ArtifactTransport get artifacts;

  ServiceDiscovery get serviceDiscovery;

  FilePicker? get filePicker => null;
}
