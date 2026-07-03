import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/navigation/app_tab.dart';
import '../../core/navigation/nav_config.dart';
import '../../core/services/app_service.dart';
import '../../core/services/workflows_service.dart';
import '../../core/utils/url_sync.dart';
import '../../core/domain/models/run/run_summary.dart';
import 'url_sync.dart';

class UrlSyncController {
  UrlSyncController(this._ref);

  final Ref _ref;
  var _applyingFromUrl = false;
  String? _pendingWorkflowId;
  String? _pendingRunId;

  bool _isKnownTab(String tab) => tabs.any((t) => t.wire == tab);

  void init() {
    applyFromUrl();
    UrlSyncBinding.instance.onPopState.listen((_) => applyFromUrl());
  }

  void writeUrl({bool replace = false}) {
    if (_applyingFromUrl) return;

    final app = _ref.read(appProvider);
    final workflows = _ref.read(workflowsProvider);
    String? id;

    if (app.activeTab == AppTab.workflows) {
      id = workflows.selectedWorkflowId;
    } else if (app.activeTab == AppTab.runs) {
      id = workflows.selectedWorkflowRunId;
    }

    final hash = formatRoute(app.activeTab.wire, id);
    if (replace) {
      // pushState is handled by setter; for replace we'd need replaceState — keep push for now.
    }
    UrlSyncBinding.instance.hash = hash;
  }

  void applyFromUrl() {
    final route = parseRoute(UrlSyncBinding.instance.hash, _isKnownTab);
    if (route.tab == null) return;

    final tab = AppTab.fromWire(route.tab);
    if (tab == null) return;

    _applyingFromUrl = true;
    try {
      _ref.read(appProvider.notifier).setActiveTab(tab);

      if (route.id != null) {
        if (tab == AppTab.workflows) {
          _selectWorkflowById(route.id!);
        } else if (tab == AppTab.runs) {
          _selectRunById(route.id!);
        }
      }
    } finally {
      _applyingFromUrl = false;
    }
  }

  void onWorkflowsChanged() {
    if (_pendingWorkflowId != null) {
      if (_selectWorkflowById(_pendingWorkflowId!)) {
        _pendingWorkflowId = null;
      }
    }
    writeUrl();
  }

  void onRunsChanged() {
    if (_pendingRunId != null) {
      if (_selectRunById(_pendingRunId!)) {
        _pendingRunId = null;
      }
    }
    writeUrl();
  }

  bool _selectWorkflowById(String id) {
    final notifier = _ref.read(workflowsProvider.notifier);
    final workflows = _ref.read(workflowsProvider).workflows;
    for (final workflow in workflows) {
      if (workflow.id == id) {
        notifier.catalog.selectWorkflow(workflow);
        return true;
      }
    }
    _pendingWorkflowId = id;
    return false;
  }

  bool _selectRunById(String id) {
    final notifier = _ref.read(workflowsProvider.notifier);
    final runs = _ref.read(workflowsProvider).workflowRuns;
    for (final run in runs) {
      if (run.id == id) {
        notifier.runs.selectWorkflowRun(run);
        return true;
      }
    }

    notifier.runs.selectWorkflowRun(RunSummary(id: id, status: '', createdAt: '', startedAt: null, finishedAt: null));
    _pendingRunId = id;
    return true;
  }
}

final urlSyncControllerProvider = Provider<UrlSyncController>((ref) => UrlSyncController(ref));
