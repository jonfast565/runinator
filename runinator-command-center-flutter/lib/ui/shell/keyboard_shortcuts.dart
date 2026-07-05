import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/navigation/app_tab.dart';
import '../../core/navigation/nav_config.dart';
import '../../core/services/app_service.dart';
import '../../core/services/resources_service.dart';
import '../../core/services/secrets_service.dart';
import '../../core/services/workflows_service.dart';

/// global keyboard shortcuts mirroring ui/composables/useKeyboardShortcuts.ts.
class CommandCenterKeyboardShortcuts extends ConsumerWidget {
  const CommandCenterKeyboardShortcuts({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Focus(
      autofocus: true,
      onKeyEvent: (node, event) {
        if (event is! KeyDownEvent) return KeyEventResult.ignored;

        final editing = _isEditableFocus(node);

        if (event.logicalKey == LogicalKeyboardKey.f5) {
          final workflows = ref.read(workflowsProvider.notifier);
          if (HardwareKeyboard.instance.isShiftPressed) {
            workflows.runs.cancelSelectedWorkflowRun();
          } else {
            workflows.runs.continueSelectedWorkflowRun();
          }
          return KeyEventResult.handled;
        }

        if (event.logicalKey == LogicalKeyboardKey.f10) {
          final workflows = ref.read(workflowsProvider.notifier);
          if (HardwareKeyboard.instance.isControlPressed) {
            final nodeId = ref.read(workflowsProvider).selectedWorkflowRunNodeId;
            if (nodeId.isNotEmpty) {
              workflows.runs.runToCursor(nodeId);
            }
          } else {
            workflows.runs.stepSelectedWorkflowRun();
          }
          return KeyEventResult.handled;
        }

        if (event.logicalKey == LogicalKeyboardKey.f9) {
          final nodeId = ref.read(workflowsProvider).selectedWorkflowRunNodeId;
          if (nodeId.isNotEmpty) {
            ref.read(workflowsProvider.notifier).runs.toggleBreakpoint(nodeId);
          }
          return KeyEventResult.handled;
        }

        if (editing) return KeyEventResult.ignored;

        final app = ref.read(appProvider);
        final tab = app.activeTab;

        if (event.logicalKey == LogicalKeyboardKey.arrowDown) {
          _moveSelection(ref, tab, 1);
          return KeyEventResult.handled;
        }

        if (event.logicalKey == LogicalKeyboardKey.arrowUp) {
          _moveSelection(ref, tab, -1);
          return KeyEventResult.handled;
        }

        if (event.logicalKey == LogicalKeyboardKey.keyR || (HardwareKeyboard.instance.isControlPressed && event.logicalKey == LogicalKeyboardKey.keyR)) {
          _refreshActive(ref, tab);
          return KeyEventResult.handled;
        }

        if (event.logicalKey == LogicalKeyboardKey.enter && tab == AppTab.workflows) {
          ref.read(workflowsProvider.notifier).runs.runSelectedWorkflow();
          return KeyEventResult.handled;
        }

        return KeyEventResult.ignored;
      },
      child: child,
    );
  }

  bool _isEditableFocus(FocusNode? focus) {
    if (focus == null) return false;
    final context = focus.context;
    if (context == null) return false;
    final widget = context.widget;
    return widget is EditableText || widget is TextField;
  }

  void _moveSelection(WidgetRef ref, AppTab tab, int delta) {
    if (tab == AppTab.workflows) {
      ref.read(workflowsProvider.notifier).catalog.moveWorkflowSelection(delta);
      return;
    }

    if (isResourceTab(tab)) {
      ref.read(resourcesProvider.notifier).moveResourceSelection(delta);
      return;
    }

    if (tab == AppTab.secrets) {
      final query = ref.read(appProvider.notifier).normalizedSearch;
      ref.read(secretsProvider.notifier).moveSecretSelection(delta, query);
    }
  }

  void _refreshActive(WidgetRef ref, AppTab tab) {
    if (tab == AppTab.runs) {
      ref.read(workflowsProvider.notifier).runs.fetchRecentWorkflowRuns();
      return;
    }

    if (tab == AppTab.workflows) {
      ref.read(workflowsProvider.notifier).catalog.refreshWorkflows();
      return;
    }

    if (tab == AppTab.secrets) {
      ref.read(secretsProvider.notifier).refreshSecrets();
      return;
    }

    final endpoint = endpointForTab(tab);
    if (endpoint != null) {
      ref.read(resourcesProvider.notifier).refreshResourcesFor(endpoint);
    }
  }
}
