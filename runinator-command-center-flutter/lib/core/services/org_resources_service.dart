// port of core/services/org-resources.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import 'app_service.dart';

part 'org_resources_service.g.dart';

class OrgResourcesService {
  const OrgResourcesService(this._app);

  final AppNotifier _app;

  Future<api.OrgNodesResponse> fetchNodes(String orgId) =>
      _app.runOperation('Loading org nodes', () => api.fetchOrgNodes(orgId));

  Future<api.OrgQuota> fetchQuota(String orgId) => api.fetchOrgQuota(orgId);

  Future<api.OrgUsage> fetchUsage(String orgId) => api.fetchOrgUsage(orgId);

  Future<api.RateCard> fetchRateCard() => api.fetchRateCard();

  Future<api.OrgResourceGroup> scaleNodes(String orgId, api.ScaleOrgNodesRequest request) =>
      _app.runOperation('Scaling org nodes', () => api.scaleOrgNodes(orgId, request));
}

@riverpod
OrgResourcesService orgResourcesService(Ref ref) => OrgResourcesService(ref.watch(appProvider.notifier));
