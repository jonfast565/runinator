// port of core/services/artifacts.ts.
//
// `File` (browser type) is represented as an opaque `Object`, same convention
// as core/platform/types.dart.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import 'app_service.dart';
import 'gates_service.dart' show ConfirmContext;

part 'artifacts_service.g.dart';

abstract class ArtifactsUploadContext {
  bool isDesktop();

  Future<Object?> pickFile();

  Future<RunArtifact> uploadFromBrowser(String runId, Object file);

  Future<RunArtifact> uploadFromPath(String runId);
}

abstract class ArtifactsDownloadContext {
  bool isDesktop();

  Future<void> downloadInBrowser(String artifactId, String name);

  Future<({String? savedTo})> downloadToPath(String artifactId, String name);
}

class ArtifactsState {
  const ArtifactsState({required this.artifacts, this.selectedArtifactId, required this.uploadRunId});

  final List<RunArtifact> artifacts;
  final String? selectedArtifactId;
  final String uploadRunId;

  ArtifactsState copyWith({List<RunArtifact>? artifacts, Object? selectedArtifactId = _unset, String? uploadRunId}) =>
      ArtifactsState(
        artifacts: artifacts ?? this.artifacts,
        selectedArtifactId: identical(selectedArtifactId, _unset) ? this.selectedArtifactId : selectedArtifactId as String?,
        uploadRunId: uploadRunId ?? this.uploadRunId,
      );
}

const Object _unset = Object();

@riverpod
class ArtifactsNotifier extends _$ArtifactsNotifier {
  @override
  ArtifactsState build() => const ArtifactsState(artifacts: [], selectedArtifactId: null, uploadRunId: '');

  RunArtifact? selectedArtifact() {
    for (final artifact in state.artifacts) {
      if (artifact.id == state.selectedArtifactId) {
        return artifact;
      }
    }
    return null;
  }

  void setSelectedArtifactId(String? id) {
    state = state.copyWith(selectedArtifactId: id);
  }

  void setUploadRunId(String runId) {
    state = state.copyWith(uploadRunId: runId);
  }

  Future<void> refreshArtifacts() async {
    List<RunArtifact> artifacts;
    try {
      artifacts = await ref.read(appProvider.notifier).runOperation('Loading artifacts', api.fetchAllArtifacts);
    } catch (_) {
      artifacts = [];
    }
    state = state.copyWith(artifacts: artifacts);
  }

  void clearArtifacts() {
    state = const ArtifactsState(artifacts: [], selectedArtifactId: null, uploadRunId: '');
  }

  String? promptForRunId(ConfirmContext confirm) {
    final app = ref.read(appProvider.notifier);
    final value = confirm.prompt('Attach artifact to which run id?');

    if (value == null || value.isEmpty) {
      return null;
    }

    final runId = value.trim();

    if (runId.isEmpty) {
      app.setError('Invalid run id');
      return null;
    }

    return runId;
  }

  Future<void> promptUploadArtifact(ArtifactsUploadContext upload, ConfirmContext confirm) async {
    final app = ref.read(appProvider.notifier);
    final uploadRunId = state.uploadRunId;

    RunArtifact? result;
    try {
      result = await app.runOperation('Uploading artifact', () async {
        final runId = uploadRunId.trim().isNotEmpty ? uploadRunId.trim() : promptForRunId(confirm);

        if (runId == null) {
          return null;
        }

        if (upload.isDesktop()) {
          return upload.uploadFromPath(runId);
        }

        final file = await upload.pickFile();

        if (file == null) {
          return null;
        }

        return upload.uploadFromBrowser(runId, file);
      });
    } catch (error) {
      app.setError(error.toString());
      result = null;
    }

    if (result != null) {
      app.setStatus('Uploaded artifact ${result.name}');
      await refreshArtifacts();
    }
  }

  Future<void> promptDownloadArtifact(RunArtifact artifact, ArtifactsDownloadContext download) async {
    final app = ref.read(appProvider.notifier);

    try {
      final info = await app.runOperation('Downloading ${artifact.name}', () async {
        if (download.isDesktop()) {
          return download.downloadToPath(artifact.id, artifact.name);
        }

        await download.downloadInBrowser(artifact.id, artifact.name);
        return (savedTo: null);
      });

      if (info.savedTo != null) {
        app.setStatus('Saved to ${info.savedTo}');
      } else {
        app.setStatus('Downloaded ${artifact.name}');
      }
    } catch (error) {
      app.setError(error.toString());
    }
  }

  Future<void> removeArtifact(RunArtifact artifact, ConfirmContext confirm) async {
    final app = ref.read(appProvider.notifier);

    if (!confirm.confirm('Delete artifact "${artifact.name}"? This also removes the stored file.')) {
      return;
    }

    try {
      await app.runOperation('Deleting ${artifact.name}', () => api.deleteArtifact(artifact.id));
    } catch (error) {
      app.setError(error.toString());
    }

    state = state.copyWith(
      artifacts: state.artifacts.where((entry) => entry.id != artifact.id).toList(),
      selectedArtifactId: state.selectedArtifactId == artifact.id ? null : state.selectedArtifactId,
    );

    await refreshArtifacts();
  }
}
