import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/api/command_center_api.dart';
import '../../core/domain/models/index.dart';
import '../../core/platform/index.dart' show getPlatformAdapter;
import '../../core/platform/types.dart';
import '../../core/services/app_service.dart';
import '../../core/services/artifacts_service.dart';
import '../../core/utils/values.dart';
import '../adapters/artifact_transport.dart' show WebArtifactsDownloadContext, WebArtifactsUploadContext, WebArtifactTransport;
import '../shared/cc_widgets.dart';
import '../shared/confirm.dart';
import '../shared/split_pane.dart';

class ArtifactsView extends ConsumerWidget {
  const ArtifactsView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(artifactsProvider);
    final notifier = ref.read(artifactsProvider.notifier);
    final query = ref.read(appProvider.notifier).normalizedSearch;
    final transport = getPlatformAdapter().artifacts;
    final upload = transport is WebArtifactTransport ? WebArtifactsUploadContext(transport) : _DesktopUploadContext(transport);
    final download = transport is WebArtifactTransport ? WebArtifactsDownloadContext(transport) : _DesktopDownloadContext(transport);
    final confirm = FlutterConfirmContext(context);

    final rows = state.artifacts.where((item) {
      if (query.isEmpty) return true;
      return [item.id, item.name, item.mimeType].any((v) => displayValue(v).toLowerCase().contains(query));
    }).toList();

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        initialFirstFraction: 0.55,
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: 'Artifacts',
                actions: [
                  CcButton(
                    icon: IconName.upload,
                    label: 'Upload',
                    dense: true,
                    onPressed: () async {
                      if (ref.read(artifactsProvider).uploadRunId.trim().isEmpty) {
                        final runId = await confirm.promptAsync('Attach artifact to which run id?');
                        if (runId == null) return;
                        notifier.setUploadRunId(runId);
                      }
                      await notifier.promptUploadArtifact(upload, confirm);
                    },
                  ),
                  CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.refreshArtifacts()),
                ],
              ),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12),
                child: TextField(
                  decoration: const InputDecoration(labelText: 'Attach to run id (optional)', isDense: true),
                  controller: TextEditingController(text: state.uploadRunId),
                  onChanged: notifier.setUploadRunId,
                ),
              ),
              const SizedBox(height: 8),
              Expanded(
                child: rows.isEmpty
                    ? const EmptyState(message: 'No artifacts.')
                    : ListView.builder(
                        itemCount: rows.length,
                        itemBuilder: (context, index) {
                          final item = rows[index];
                          return ListTile(
                            selected: item.id == state.selectedArtifactId,
                            title: Text(item.name),
                            subtitle: Text('${item.mimeType} · ${displayValue(item.sizeBytes)} bytes'),
                            onTap: () => notifier.setSelectedArtifactId(item.id),
                            trailing: Wrap(
                              spacing: 4,
                              children: [
                                IconButton(
                                  icon: const Icon(Icons.download, size: 16),
                                  onPressed: () => notifier.promptDownloadArtifact(item, download),
                                ),
                                IconButton(
                                  icon: const Icon(Icons.delete_outline, size: 16),
                                  onPressed: () async {
                                    if (!await confirm.confirmAsync('Delete artifact "${item.name}"?')) return;
                                    await notifier.removeArtifact(item, confirm);
                                  },
                                ),
                              ],
                            ),
                          );
                        },
                      ),
              ),
            ],
          ),
        ),
        second: PanelCard(
          child: Builder(
            builder: (context) {
              final selected = notifier.selectedArtifact();
              if (selected == null) {
                return const EmptyState(message: 'Select an artifact.');
              }
              return Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(selected.name, style: const TextStyle(fontWeight: FontWeight.w700)),
                  Text('ID: ${selected.id}', style: const TextStyle(fontSize: 11)),
                  Text('MIME: ${selected.mimeType}'),
                  Text('Size: ${displayValue(selected.sizeBytes)} bytes'),
                  if (selected.uri != null) Text('URI: ${selected.uri}', style: const TextStyle(fontSize: 11)),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

class _DesktopUploadContext implements ArtifactsUploadContext {
  _DesktopUploadContext(this._transport);

  final dynamic _transport;

  @override
  bool isDesktop() => _transport.isDesktop();

  @override
  Future<Object?> pickFile() => _transport.pickFile();

  @override
  Future<RunArtifact> uploadFromBrowser(String runId, Object file) => _transport.uploadFromBrowser(ArtifactUploadRequest(runId: runId), file);

  @override
  Future<RunArtifact> uploadFromPath(String runId) => _transport.uploadFromPath(ArtifactUploadRequest(runId: runId));
}

class _DesktopDownloadContext implements ArtifactsDownloadContext {
  _DesktopDownloadContext(this._transport);

  final dynamic _transport;

  @override
  bool isDesktop() => _transport.isDesktop();

  @override
  Future<void> downloadInBrowser(String artifactId, String name) => _transport.downloadInBrowser(artifactId, name);

  @override
  Future<({String? savedTo})> downloadToPath(String artifactId, String name) async {
    final result = await _transport.downloadToPath(artifactId, name);
    return (savedTo: result.savedTo as String?);
  }
}
