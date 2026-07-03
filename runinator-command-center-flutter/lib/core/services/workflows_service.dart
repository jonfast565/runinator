// port of core/services/workflows/index.ts's createWorkflowServices(). wires
// the shared WorkflowServiceHost together with the catalog/editor/runs
// "sub-services", exactly mirroring the source's host + catalogPeer wiring
// (including the same lazy-bound catalogPeer trick to avoid a circular
// dependency between editor and catalog).

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../platform/downloads.dart' show downloadBlob, downloadTextFile;
import '../workflow/workflow_helpers.dart' show workflowNodeKinds, directTransitionKeys;
import 'app_service.dart';
import 'providers_service.dart';
import 'resources_service.dart';
import 'workflows/catalog.dart';
import 'workflows/editor.dart';
import 'workflows/host.dart';
import 'workflows/runs.dart';
import 'workflows/state.dart';

part 'workflows_service.g.dart';

class _CatalogPeer implements WorkflowCatalogPeer {
  Future<void> Function()? _saveSelectedWorkflowBundle;

  @override
  Future<void> saveSelectedWorkflowBundle() => _saveSelectedWorkflowBundle?.call() ?? Future.value();
}

@riverpod
class WorkflowsNotifier extends _$WorkflowsNotifier {
  late final WorkflowServiceHost _host;
  late final WorkflowRunService runs;
  late final WorkflowEditorService editor;
  late final WorkflowCatalogService catalog;

  @override
  WorkflowServicesState build() {
    final initialState = createWorkflowServicesState();
    WorkflowServicesState current = initialState;

    _host = WorkflowServiceHost(
      deps: WorkflowServiceDeps(
        app: ref.watch(appProvider.notifier),
        getProviders: () => ref.read(providersProvider).providers,
        refreshResources: () => ref.read(resourcesProvider.notifier).refreshResources(),
        downloadTextFile: downloadTextFile,
        downloadBlob: downloadBlob,
      ),
      internal: WorkflowServicesInternal(),
      getState: () => current,
      setState: (next) {
        current = next;
        state = next;
      },
    );

    runs = WorkflowRunService(_host);
    final catalogPeer = _CatalogPeer();
    editor = WorkflowEditorService(_host, runs, catalogPeer);
    catalog = WorkflowCatalogService(_host, editor, runs);
    catalogPeer._saveSelectedWorkflowBundle = catalog.saveSelectedWorkflowBundle;

    return initialState;
  }

  List<String> get nodeKinds => workflowNodeKinds;

  List<String> get directTransitions => directTransitionKeys;

  WorkflowServiceHost get host => _host;
}
