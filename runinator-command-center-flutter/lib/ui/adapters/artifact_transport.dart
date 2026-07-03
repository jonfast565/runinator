import 'package:dio/dio.dart';
import 'package:file_picker/file_picker.dart' as fp;

import '../../core/api/command_center_api.dart';
import '../../core/api/http_runtime.dart';
import '../../core/domain/models/index.dart';
import '../../core/platform/types.dart' hide FilePicker;
import '../../core/services/artifacts_service.dart';

Map<String, String> _artifactAuthHeaders() {
  final token = httpAuthToken();
  return token != null ? {'authorization': 'Bearer $token'} : {};
}

class WebArtifactTransport implements ArtifactTransport {
  final Dio _dio = Dio();

  @override
  bool isDesktop() => false;

  @override
  Future<Object?> pickFile() async {
    final result = await fp.FilePicker.platform.pickFiles(withData: true);
    if (result == null || result.files.isEmpty) return null;
    return result.files.single;
  }

  @override
  Future<RunArtifact> uploadFromPath(ArtifactUploadRequest request) =>
      throw UnsupportedError('uploadFromPath requires desktop');

  @override
  Future<RunArtifact> uploadFromBrowser(ArtifactUploadRequest request, Object file) async {
    if (file is! fp.PlatformFile) {
      throw ArgumentError('Expected PlatformFile');
    }

    final bytes = file.bytes;
    if (bytes == null) {
      throw StateError('Selected file has no bytes');
    }

    final form = FormData.fromMap({
      'run_id': request.runId,
      'name': file.name,
      'mime_type': file.extension != null ? 'application/${file.extension}' : 'application/octet-stream',
      if (request.workflowNodeRunId != null) 'workflow_node_run_id': request.workflowNodeRunId,
      'file': MultipartFile.fromBytes(bytes, filename: file.name),
    });

    final response = await _dio.post<Object?>(
      '${apiBaseUrl()}/artifacts/upload',
      data: form,
      options: Options(headers: _artifactAuthHeaders()),
    );

    return RunArtifact.fromJson(response.data as Map<String, Object?>);
  }

  @override
  Future<void> downloadInBrowser(String artifactId, String name) async {
    await _dio.get<List<int>>(
      '${apiBaseUrl()}/artifacts/$artifactId/download',
      options: Options(responseType: ResponseType.bytes, headers: _artifactAuthHeaders()),
    );
  }

  @override
  Future<ArtifactDownloadResult> downloadToPath(String artifactId, String name) async =>
      const ArtifactDownloadResult(savedTo: null);
}

class WebArtifactsUploadContext implements ArtifactsUploadContext {
  WebArtifactsUploadContext(this._transport);

  final WebArtifactTransport _transport;

  @override
  bool isDesktop() => false;

  @override
  Future<Object?> pickFile() => _transport.pickFile();

  @override
  Future<RunArtifact> uploadFromBrowser(String runId, Object file) =>
      _transport.uploadFromBrowser(ArtifactUploadRequest(runId: runId), file);

  @override
  Future<RunArtifact> uploadFromPath(String runId) => _transport.uploadFromPath(ArtifactUploadRequest(runId: runId));
}

class WebArtifactsDownloadContext implements ArtifactsDownloadContext {
  WebArtifactsDownloadContext(this._transport);

  final WebArtifactTransport _transport;

  @override
  bool isDesktop() => false;

  @override
  Future<void> downloadInBrowser(String artifactId, String name) =>
      _transport.downloadInBrowser(artifactId, name);

  @override
  Future<({String? savedTo})> downloadToPath(String artifactId, String name) async =>
      (savedTo: (await _transport.downloadToPath(artifactId, name)).savedTo);
}
