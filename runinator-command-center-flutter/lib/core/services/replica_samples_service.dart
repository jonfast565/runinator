// port of core/services/replica-samples.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart';
import 'app_service.dart';

part 'replica_samples_service.g.dart';

class ReplicaSamplesService {
  const ReplicaSamplesService(this._app);

  final AppNotifier _app;

  Future<ReplicaSampleSeries> fetch(String replicaId, [int? sinceSeconds]) async {
    try {
      return await _app.runOperation(
        'Loading replica samples',
        () => fetchReplicaSamples(replicaId, sinceSeconds),
      );
    } catch (_) {
      return ReplicaSampleSeries(replicaId: replicaId, samples: const []);
    }
  }
}

@riverpod
ReplicaSamplesService replicaSamplesService(Ref ref) => ReplicaSamplesService(ref.watch(appProvider.notifier));
