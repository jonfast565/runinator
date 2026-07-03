// port of core/navigation/app.ts.

import '../domain/icons.dart';

enum AppTab {
  dev('Dev'),
  workflows('Workflows'),
  runs('Runs'),
  providers('Providers'),
  replicas('Replicas'),
  approvals('Approvals'),
  artifacts('Artifacts'),
  notifications('Notifications'),
  events('Events'),
  externalItems('ExternalItems'),
  gates('Gates'),
  configs('Configs'),
  secrets('Secrets'),
  adminSettings('AdminSettings'),
  permissions('Permissions'),
  deadLetters('DeadLetters'),
  auditLog('AuditLog'),
  organization('Organization'),
  orgResources('OrgResources');

  const AppTab(this.wire);

  final String wire;

  static AppTab? fromWire(String? value) {
    if (value == null) {
      return null;
    }

    for (final tab in AppTab.values) {
      if (tab.wire == value) {
        return tab;
      }
    }

    return null;
  }
}

class ResourceEndpoint {
  const ResourceEndpoint({required this.label, required this.endpoint});

  final String label;
  final String endpoint;
}

class NavItem {
  const NavItem({
    required this.tab,
    required this.label,
    required this.icon,
    this.endpoint,
    // only available in the tauri desktop client; hidden in the hosted web app.
    this.desktopOnly = false,
    // only available to admins, or to auth-disabled stacks where every caller is an admin.
    this.adminOnly = false,
    // placeholder for the global search box; when set the tab's list consumes app.searchQuery.
    // when unset the search box is hidden for this tab so it is never a dead control.
    this.searchPlaceholder,
  });

  final AppTab tab;
  final String label;
  final IconName icon;
  final String? endpoint;
  final bool desktopOnly;
  final bool adminOnly;
  final String? searchPlaceholder;
}

class NavSection {
  const NavSection({required this.label, required this.items});

  final String label;
  final List<NavItem> items;
}
