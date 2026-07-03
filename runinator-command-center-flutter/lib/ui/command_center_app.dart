import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/api/http_runtime.dart';
import '../core/navigation/app_tab.dart';
import '../core/navigation/nav_config.dart';
import '../core/platform/index.dart';
import '../core/realtime/event_router.dart';
import '../core/realtime/event_stream_client.dart';
import '../core/services/admin_settings_service.dart';
import '../core/services/app_service.dart';
import '../core/services/artifacts_service.dart';
import '../core/services/auth_service.dart';
import '../core/services/display_preferences_service.dart';
import '../core/services/gates_service.dart';
import '../core/services/notifications_service.dart';
import '../core/services/orgs_service.dart';
import '../core/services/permissions_service.dart';
import '../core/services/providers_service.dart';
import '../core/services/resources_service.dart';
import '../core/services/secrets_service.dart';
import '../core/services/workflows_service.dart';
import 'adapters/url_sync_controller.dart';
import 'shell/app_shell.dart';
import 'theme/app_theme.dart';
import 'views/admin_settings_view.dart';
import 'views/admin_views.dart';
import 'views/artifacts_view.dart';
import 'views/gates_view.dart';
import 'views/login_view.dart';
import 'views/notifications_view.dart';
import 'views/dev_view.dart';
import 'views/org_resources_view.dart';
import 'views/organization_view.dart';
import 'views/permissions_view.dart';
import 'views/providers_view.dart';
import 'views/replicas_view.dart';
import 'views/resources_view.dart';
import 'views/runs_view.dart';
import 'views/secrets_view.dart';
import 'views/workflows_view.dart';

class CommandCenterApp extends ConsumerWidget {
  const CommandCenterApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final display = ref.watch(displayPreferencesProvider);

    return MaterialApp(
      title: 'Runinator Command Center',
      theme: buildAppTheme(brightness: Brightness.light),
      darkTheme: buildAppTheme(brightness: Brightness.dark),
      themeMode: themeModeFor(display.theme),
      home: const CommandCenterRoot(),
      debugShowCheckedModeBanner: false,
    );
  }
}

class CommandCenterRoot extends ConsumerStatefulWidget {
  const CommandCenterRoot({super.key});

  @override
  ConsumerState<CommandCenterRoot> createState() => _CommandCenterRootState();
}

class _CommandCenterRootState extends ConsumerState<CommandCenterRoot> {
  Timer? _healthTimer;
  Timer? _replicaTimer;
  EventStreamClient? _eventStream;
  var _bootstrapped = false;
  var _urlSyncReady = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _bootstrapOnce());
  }

  @override
  void dispose() {
    _healthTimer?.cancel();
    _replicaTimer?.cancel();
    _eventStream?.disconnect();
    super.dispose();
  }

  Future<void> _bootstrapOnce() async {
    if (_bootstrapped) return;
    _bootstrapped = true;

    final discovery = getPlatformAdapter().serviceDiscovery;
    final app = ref.read(appProvider.notifier);
    final baseUrl = discovery.webServiceUrl();
    app.setServiceUrl(baseUrl.isEmpty ? null : baseUrl);

    if (baseUrl.isEmpty) {
      app.setError('No service URL configured. Set RUNINATOR_WS_URL or serve behind /api proxy.');
      app.setInitialLoading(false);
      return;
    }

    await ref.read(authProvider.notifier).init();
    if (ref.read(authProvider).authenticated) {
      await _refreshBackendState(refreshProviders: true);
      _startEventStream();
    }
    app.setInitialLoading(false);
    _initUrlSync();

    _healthTimer = Timer.periodic(const Duration(seconds: 10), (_) async {
      if (await pingBackendHealth()) {
        ref.read(appProvider.notifier).markBackendReachable();
      } else {
        ref.read(appProvider.notifier).markBackendUnreachable();
      }
    });

    _replicaTimer = Timer.periodic(const Duration(seconds: 15), (_) {
      if (ref.read(appProvider).serviceUrl != null) {
        ref.read(appProvider.notifier).refreshReplicas().catchError((_) {});
      }
    });
  }

  void _initUrlSync() {
    if (_urlSyncReady) return;
    _urlSyncReady = true;
    ref.read(urlSyncControllerProvider).init();
  }

  void _startEventStream() {
    _eventStream?.disconnect();
    final router = createEventStreamRouter(() {
      final app = ref.read(appProvider);
      final workflows = ref.read(workflowsProvider);
      return EventStreamRouterDeps(
        activeTab: app.activeTab.wire,
        selectedWorkflowRunId: workflows.selectedWorkflowRunId,
        isWorkflowEditorDirty: workflows.isDirty,
        refreshResourcesIfActive: () {
          final tab = ref.read(appProvider).activeTab;
          final endpoint = endpointForTab(tab);
          if (endpoint != null && isResourceTab(tab)) {
            ref.read(resourcesProvider.notifier).refreshResourcesFor(endpoint).catchError((_) {});
          }
        },
        refreshActiveState: () => _refreshBackendState(refreshProviders: false),
        refreshWorkflowsIfClean: () {
          if (!ref.read(workflowsProvider).isDirty) {
            ref.read(workflowsProvider.notifier).catalog.refreshWorkflows().catchError((_) {});
          }
        },
        refreshRecentRunsIfActive: () {
          if (ref.read(appProvider).activeTab == AppTab.runs) {
            ref.read(workflowsProvider.notifier).runs.fetchRecentWorkflowRuns().catchError((_) {});
          }
        },
        refreshWorkflowRunIfSelected: (runId) {
          ref.read(workflowsProvider.notifier).runs.fetchWorkflowRunDetail(runId, silent: true).catchError((_) {});
        },
        refreshArtifactsIfActive: () {
          if (ref.read(appProvider).activeTab == AppTab.artifacts) {
            ref.read(artifactsProvider.notifier).refreshArtifacts().catchError((_) {});
          }
        },
        refreshNotifications: () => ref.read(notificationsProvider.notifier).refreshNotifications().catchError((_) {}),
      );
    });

    _eventStream = EventStreamClient(
      EventStreamClientOptions(
        getServiceUrl: () => ref.read(appProvider).serviceUrl,
        getServiceKnown: () => ref.read(appProvider).serviceUrl != null,
        onStateChange: (state) => ref.read(appProvider.notifier).setEventStreamState(state),
        onFallbackTick: () => _refreshBackendState(refreshProviders: false),
        router: router,
      ),
    );
    _eventStream!.connect();
  }

  Future<void> _refreshBackendState({required bool refreshProviders}) async {
    await Future.wait([
      ref.read(workflowsProvider.notifier).catalog.refreshWorkflows().catchError((_) {}),
      ref.read(workflowsProvider.notifier).runs.fetchRecentWorkflowRuns().catchError((_) {}),
      ref.read(resourcesProvider.notifier).refreshResources().catchError((_) {}),
      ref.read(notificationsProvider.notifier).refreshNotifications().catchError((_) {}),
      ref.read(secretsProvider.notifier).refreshSecrets().catchError((_) {}),
      ref.read(gatesProvider.notifier).refreshGates().catchError((_) {}),
      ref.read(appProvider.notifier).refreshReplicas().catchError((_) {}),
      ref.read(orgsProvider.notifier).refresh().catchError((_) {}),
      if (refreshProviders) ref.read(providersProvider.notifier).fetchProvidersList().catchError((_) {}),
    ]);
  }

  Future<void> _refreshTenantScopedState() async {
    ref.read(workflowsProvider.notifier).catalog.clearServiceState(discardDraft: true);
    ref.read(resourcesProvider.notifier).clearResources();
    ref.read(artifactsProvider.notifier).clearArtifacts();
    ref.read(notificationsProvider.notifier).clearNotifications();
    ref.read(secretsProvider.notifier).clearSecrets();
    ref.read(permissionsProvider.notifier).clearPermissions();
    ref.read(gatesProvider.notifier).clearGates();
    ref.read(providersProvider.notifier).clearProviders();
    ref.read(appProvider.notifier).clearReplicaState();
    await _refreshBackendState(refreshProviders: true);
  }

  void _onTabActivated(AppTab tab) {
    switch (tab) {
      case AppTab.workflows:
        if (!ref.read(workflowsProvider).isDirty) {
          ref.read(workflowsProvider.notifier).catalog.refreshWorkflows().catchError((_) {});
        }
      case AppTab.runs:
        ref.read(workflowsProvider.notifier).runs.fetchRecentWorkflowRuns().catchError((_) {});
      case AppTab.replicas:
        ref.read(appProvider.notifier).refreshReplicas().catchError((_) {});
      case AppTab.configs:
      case AppTab.secrets:
        ref.read(secretsProvider.notifier).refreshSecrets().catchError((_) {});
      case AppTab.adminSettings:
        ref.read(adminSettingsProvider.notifier).refresh().catchError((_) {});
      case AppTab.artifacts:
        ref.read(artifactsProvider.notifier).refreshArtifacts().catchError((_) {});
      case AppTab.notifications:
        ref.read(notificationsProvider.notifier).refreshNotifications().catchError((_) {});
      case AppTab.permissions:
        ref.read(permissionsProvider.notifier).refreshAll().catchError((_) {});
      default:
        final endpoint = endpointForTab(tab);
        if (endpoint != null && isResourceTab(tab)) {
          ref.read(resourcesProvider.notifier).refreshResourcesFor(endpoint).catchError((_) {});
        }
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(authProvider.select((s) => s.authenticated), (prev, next) {
      if (next && ref.read(appProvider).serviceUrl != null) {
        _refreshBackendState(refreshProviders: true);
        _startEventStream();
      }
    });

    ref.listen(orgsProvider.select((s) => s.activeOrgId), (prev, next) {
      if (next != null && next != prev && ref.read(authProvider).authenticated) {
        _refreshTenantScopedState();
      }
    });

    ref.listen(appProvider.select((s) => s.activeTab), (prev, next) {
      if (prev != next) {
        ref.read(appProvider.notifier).setSearchQuery('');
        _onTabActivated(next);
        ref.read(urlSyncControllerProvider).writeUrl();
      }
    });

    ref.listen(workflowsProvider.select((s) => s.selectedWorkflowId), (prev, next) {
      if (prev != next) {
        ref.read(urlSyncControllerProvider).onWorkflowsChanged();
      }
    });

    ref.listen(workflowsProvider.select((s) => s.selectedWorkflowRunId), (prev, next) {
      if (prev != next) {
        ref.read(urlSyncControllerProvider).onRunsChanged();
      }
    });

    final auth = ref.watch(authProvider);
    final app = ref.watch(appProvider);

    if (auth.required && !auth.authenticated) {
      return const LoginView();
    }

    if (app.initialLoading) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }

    return AppShellWithToasts(child: _ActiveTabView(tab: app.activeTab));
  }
}

class _ActiveTabView extends StatelessWidget {
  const _ActiveTabView({required this.tab});

  final AppTab tab;

  @override
  Widget build(BuildContext context) {
    return switch (tab) {
      AppTab.dev => const DevView(),
      AppTab.workflows => const WorkflowsView(),
      AppTab.runs => const RunsView(),
      AppTab.providers => const ProvidersView(),
      AppTab.replicas => const ReplicasView(),
      AppTab.approvals => const ApprovalsView(),
      AppTab.notifications => const NotificationsView(),
      AppTab.artifacts => const ArtifactsView(),
      AppTab.events => const EventsView(),
      AppTab.externalItems => const ExternalItemsView(),
      AppTab.gates => const GatesView(),
      AppTab.configs => const SecretsView(kind: SettingKindView.config),
      AppTab.secrets => const SecretsView(kind: SettingKindView.secret),
      AppTab.organization => const OrganizationView(),
      AppTab.orgResources => const OrgResourcesView(),
      AppTab.adminSettings => const AdminSettingsView(),
      AppTab.permissions => const PermissionsView(),
      AppTab.deadLetters => const DeadLettersView(),
      AppTab.auditLog => const AuditLogView(),
    };
  }
}
