import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/navigation/app_tab.dart';
import '../../core/navigation/breakpoints.dart';
import '../../core/navigation/nav_config.dart';
import '../../core/services/app_service.dart';
import '../../core/services/auth_service.dart';
import '../../core/services/orgs_service.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';

class TopToolbar extends ConsumerWidget {
  const TopToolbar({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final app = ref.watch(appProvider);
    final auth = ref.watch(authProvider);
    final orgs = ref.watch(orgsProvider);
    final navItem = navItemForTab(app.activeTab);
    final placeholder = navItem?.searchPlaceholder;
    final isMobile = MediaQuery.sizeOf(context).width <= Breakpoints.mobile;

    return Container(
      padding: const EdgeInsets.fromLTRB(12, 12, 16, 10),
      decoration: BoxDecoration(color: AppColors.surface, border: Border(bottom: BorderSide(color: AppColors.border))),
      child: Row(
        children: [
          if (isMobile) ...[
            IconButton(
              icon: const Icon(Icons.menu),
              tooltip: 'Menu',
              onPressed: () => Scaffold.of(context).openDrawer(),
            ),
            const SizedBox(width: 4),
          ],
          Flexible(
            child: Text(
              navItem?.label ?? app.activeTab.wire,
              style: Theme.of(context).textTheme.titleMedium,
              overflow: TextOverflow.ellipsis,
              maxLines: 1,
            ),
          ),
          const SizedBox(width: 16),
          if (placeholder != null)
            Expanded(
              child: TextField(
                decoration: InputDecoration(
                  hintText: placeholder,
                  prefixIcon: const CcIcon(IconName.search, size: 16),
                  isDense: true,
                ),
                onChanged: (value) => ref.read(appProvider.notifier).setSearchQuery(value),
              ),
            )
          else
            const Spacer(),
          if (orgs.memberships.length > 1)
            Flexible(
              child: Padding(
                padding: const EdgeInsets.only(right: 8),
                child: DropdownButton<String>(
                  isExpanded: true,
                  value: orgs.activeOrgId,
                  items: [
                    for (final membership in orgs.memberships)
                      DropdownMenuItem(
                        value: membership.org.id,
                        child: Text(membership.org.name, overflow: TextOverflow.ellipsis),
                      ),
                  ],
                  onChanged: (value) {
                    if (value != null) {
                      ref.read(orgsProvider.notifier).setActive(value);
                    }
                  },
                ),
              ),
            ),
          CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => _refreshActiveTab(ref, app.activeTab)),
          const SizedBox(width: 8),
          if (auth.user != null)
            Flexible(
              child: PopupMenuButton<String>(
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 8),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      const CcIcon(IconName.user, size: 16),
                      const SizedBox(width: 6),
                      Flexible(
                        child: Text(
                          auth.user?['username']?.toString() ?? 'User',
                          style: const TextStyle(fontSize: 12),
                          overflow: TextOverflow.ellipsis,
                          maxLines: 1,
                        ),
                      ),
                    ],
                  ),
                ),
                itemBuilder: (context) => [
                  PopupMenuItem(value: 'signout', child: const Text('Sign out')),
                ],
                onSelected: (value) {
                  if (value == 'signout') {
                    ref.read(authProvider.notifier).signOut();
                  }
                },
              ),
            ),
        ],
      ),
    );
  }

  void _refreshActiveTab(WidgetRef ref, AppTab tab) {
    // tab-specific refresh is handled by CommandCenterRoot watchers; trigger generic refresh here.
    ref.read(appProvider.notifier).refreshReplicas().catchError((_) {});
  }
}
