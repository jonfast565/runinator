// port of core/realtime/event-router.ts.

typedef ServerEvent = Map<String, Object?>;

abstract class EventStreamRouter {
  void route(ServerEvent event);
}

class EventStreamRouterDeps {
  const EventStreamRouterDeps({
    required this.activeTab,
    required this.selectedWorkflowRunId,
    required this.isWorkflowEditorDirty,
    required this.refreshResourcesIfActive,
    required this.refreshActiveState,
    required this.refreshWorkflowsIfClean,
    required this.refreshRecentRunsIfActive,
    required this.refreshWorkflowRunIfSelected,
    required this.refreshArtifactsIfActive,
    required this.refreshNotifications,
  });

  final String activeTab;
  final String? selectedWorkflowRunId;
  final bool isWorkflowEditorDirty;
  final void Function() refreshResourcesIfActive;
  final void Function() refreshActiveState;
  final void Function() refreshWorkflowsIfClean;
  final void Function() refreshRecentRunsIfActive;
  final void Function(String runId) refreshWorkflowRunIfSelected;
  final void Function() refreshArtifactsIfActive;
  final void Function() refreshNotifications;
}

class _EventStreamRouter implements EventStreamRouter {
  _EventStreamRouter(this._deps);

  final EventStreamRouterDeps Function() _deps;

  @override
  void route(ServerEvent event) {
    final context = _deps();

    switch (event['type']) {
      case 'run_status_changed':
        final selectedId = context.selectedWorkflowRunId;
        if (selectedId != null) {
          context.refreshWorkflowRunIfSelected(selectedId);
        }
        context.refreshResourcesIfActive();
        break;
      case 'resync':
        context.refreshActiveState();
        break;
      case 'tasks_changed':
        break;
      case 'workflows_changed':
        context.refreshWorkflowsIfClean();
        break;
      case 'workflow_run_changed':
        final runId = event['run_id'] as String;
        if (context.selectedWorkflowRunId == runId) {
          context.refreshWorkflowRunIfSelected(runId);
        }
        context.refreshRecentRunsIfActive();
        context.refreshResourcesIfActive();
        break;
      case 'resources_changed':
        context.refreshResourcesIfActive();
        break;
      case 'artifact_created':
      case 'artifacts_changed':
        context.refreshArtifactsIfActive();
        break;
      case 'notification_created':
      case 'notifications_changed':
        context.refreshNotifications();
        break;
    }
  }
}

EventStreamRouter createEventStreamRouter(EventStreamRouterDeps Function() deps) =>
    _EventStreamRouter(deps);
