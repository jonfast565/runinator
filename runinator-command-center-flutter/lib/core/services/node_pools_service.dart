// port of core/services/node-pools.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import 'app_service.dart';

part 'node_pools_service.g.dart';

class NodePoolsService {
  const NodePoolsService(this._app);

  final AppNotifier _app;

  Future<api.NodeBackendsResponse> fetchBackends() =>
      _app.runOperation('Loading node backends', api.fetchNodeBackends);

  Future<List<api.ProvisionedGroup>> fetchNodes() => _app.runOperation('Loading node pools', api.fetchNodes);

  Future<api.ProvisionedGroup> scale(api.ScaleNodesRequest request) =>
      _app.runOperation('Scaling node pool', () => api.scaleNodes(request));
}

@riverpod
NodePoolsService nodePoolsService(Ref ref) => NodePoolsService(ref.watch(appProvider.notifier));
