// port of core/navigation/nav-config.ts.

import '../domain/icons.dart';
import 'app_tab.dart';

final List<NavSection> navSections = [
  const NavSection(
    label: 'Workspace',
    items: [
      NavItem(tab: AppTab.dev, label: 'Dev', icon: IconName.debug, desktopOnly: true),
      NavItem(
        tab: AppTab.workflows,
        label: 'Workflows',
        icon: IconName.workflow,
        searchPlaceholder: 'Search workflows',
      ),
      NavItem(tab: AppTab.runs, label: 'Runs', icon: IconName.runs, searchPlaceholder: 'Search runs'),
      NavItem(
        tab: AppTab.providers,
        label: 'Providers',
        icon: IconName.box,
        searchPlaceholder: 'Search providers',
      ),
      NavItem(
        tab: AppTab.replicas,
        label: 'Replicas',
        icon: IconName.list,
        searchPlaceholder: 'Search replicas',
      ),
    ],
  ),
  const NavSection(
    label: 'Inbox',
    items: [
      NavItem(
        tab: AppTab.approvals,
        label: 'Approvals',
        icon: IconName.approve,
        endpoint: 'approvals',
        searchPlaceholder: 'Search approvals',
      ),
      NavItem(
        tab: AppTab.notifications,
        label: 'Notifications',
        icon: IconName.bell,
        endpoint: 'notifications',
        searchPlaceholder: 'Search notifications',
      ),
    ],
  ),
  const NavSection(
    label: 'Data',
    items: [
      NavItem(
        tab: AppTab.artifacts,
        label: 'Artifacts',
        icon: IconName.box,
        endpoint: 'artifacts',
        searchPlaceholder: 'Search artifacts',
      ),
      NavItem(
        tab: AppTab.externalItems,
        label: 'External Items',
        icon: IconName.tag,
        endpoint: 'external_items',
        searchPlaceholder: 'Search external items',
      ),
      NavItem(
        tab: AppTab.events,
        label: 'Events',
        icon: IconName.flag,
        endpoint: 'automation_events',
        searchPlaceholder: 'Search events',
      ),
    ],
  ),
  const NavSection(
    label: 'Other',
    items: [
      NavItem(tab: AppTab.gates, label: 'Gates', icon: IconName.gate, searchPlaceholder: 'Search gates'),
      NavItem(
        tab: AppTab.configs,
        label: 'Configs',
        icon: IconName.settings,
        searchPlaceholder: 'Search configs',
      ),
      NavItem(
        tab: AppTab.secrets,
        label: 'Secrets',
        icon: IconName.key,
        searchPlaceholder: 'Search secrets',
      ),
    ],
  ),
  const NavSection(
    label: 'Organization',
    items: [
      NavItem(tab: AppTab.organization, label: 'Organization', icon: IconName.shield),
      NavItem(tab: AppTab.orgResources, label: 'Resources & Billing', icon: IconName.box),
    ],
  ),
  const NavSection(
    label: 'Admin',
    items: [
      NavItem(tab: AppTab.adminSettings, label: 'Settings', icon: IconName.settings, adminOnly: true),
      NavItem(
        tab: AppTab.permissions,
        label: 'Permissions',
        icon: IconName.shield,
        adminOnly: true,
        searchPlaceholder: 'Search users & teams',
      ),
      NavItem(tab: AppTab.deadLetters, label: 'Dead Letters', icon: IconName.flag, adminOnly: true),
      NavItem(tab: AppTab.auditLog, label: 'Audit Log', icon: IconName.list, adminOnly: true),
    ],
  ),
];

final List<AppTab> tabs = navSections.expand((section) => section.items.map((item) => item.tab)).toList();

final Map<AppTab, NavItem> _navItemByTab = {
  for (final section in navSections)
    for (final item in section.items) item.tab: item,
};

NavItem? navItemForTab(AppTab tab) => _navItemByTab[tab];

String? endpointForTab(AppTab tab) => _navItemByTab[tab]?.endpoint;

bool isResourceTab(AppTab tab) {
  final endpoint = endpointForTab(tab);

  if (endpoint == null) {
    return false;
  }

  return endpoint != 'artifacts' && endpoint != 'notifications';
}

List<NavSection> visibleNavSections({required bool canSeeAdmin, required bool isDesktop}) {
  final sections = navSections
      .map((section) => NavSection(
            label: section.label,
            items: section.items.where((item) => !item.adminOnly || canSeeAdmin).toList(),
          ))
      .where((section) => section.items.isNotEmpty)
      .toList();

  if (isDesktop) {
    return sections;
  }

  return sections
      .map((section) => NavSection(
            label: section.label,
            items: section.items.where((item) => !item.desktopOnly).toList(),
          ))
      .where((section) => section.items.isNotEmpty)
      .toList();
}

/// core/ has no browser localStorage dependency; a concrete web platform adapter
/// (future UI pass) supplies one via [setNavStorageReader]. with none configured
/// (as in a `dart test` run) this behaves like the ts source's try/catch fallback
/// path for unavailable storage.
String? Function(String key)? _storageReader;

void setNavStorageReader(String? Function(String key) reader) {
  _storageReader = reader;
}

AppTab readStoredDefaultTab() {
  final stored = _storageReader?.call('command-center.defaultTab');

  if (stored != null && tabs.any((tab) => tab.wire == stored)) {
    return AppTab.fromWire(stored)!;
  }

  return AppTab.workflows;
}

bool readSidebarCollapsed() => _storageReader?.call('command-center.sidebar.collapsed') == 'true';
