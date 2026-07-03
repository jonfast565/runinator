import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/app_service.dart';
import '../../core/services/permissions_service.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/split_pane.dart';

class PermissionsView extends ConsumerStatefulWidget {
  const PermissionsView({super.key});

  @override
  ConsumerState<PermissionsView> createState() => _PermissionsViewState();
}

class _PermissionsViewState extends ConsumerState<PermissionsView> {
  var _tab = 0;

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(permissionsProvider);
    final notifier = ref.read(permissionsProvider.notifier);
    final app = ref.watch(appProvider);
    final query = ref.read(appProvider.notifier).normalizedSearch;

    return Padding(
      padding: const EdgeInsets.all(12),
      child: PanelCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PanelToolbar(
              title: 'Permissions',
              actions: [
                CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.refreshAll()),
              ],
            ),
            TabBar(
              onTap: (index) => setState(() => _tab = index),
              tabs: const [
                Tab(text: 'Users'),
                Tab(text: 'Teams'),
                Tab(text: 'Access'),
                Tab(text: 'API Keys'),
              ],
            ),
            Expanded(
              child: switch (_tab) {
                0 => CcDataTable(
                    columns: const ['Username', 'Email', 'Admin', 'Disabled'],
                    rows: [
                      for (final user in state.users.where((u) => query.isEmpty || [u.username, u.email ?? ''].any((v) => v.toLowerCase().contains(query))))
                        [user.username, user.email ?? '', user.isAdmin ? 'yes' : 'no', user.disabled ? 'yes' : 'no'],
                    ],
                    emptyMessage: 'No users.',
                  ),
                1 => CcDataTable(
                    columns: const ['Team', 'Created'],
                    rows: [
                      for (final team in state.teams)
                        [team.name, team.createdAt],
                    ],
                    emptyMessage: 'No teams.',
                  ),
                2 => CcDataTable(
                    columns: const ['Resource', 'Principal', 'Permission'],
                    rows: [
                      for (final grant in state.workflowGrants)
                        ['${grant.resourceType}:${grant.resourceId}', '${grant.principalType.name}:${grant.principalId}', grant.permission.name],
                    ],
                    emptyMessage: 'No workflow grants.',
                  ),
                _ => CcDataTable(
                    columns: const ['Name', 'Prefix', 'Disabled'],
                    rows: [
                      for (final key in state.apiKeys)
                        [key.name, key.keyPrefix, key.disabled ? 'yes' : 'no'],
                    ],
                    emptyMessage: 'No API keys.',
                  ),
              },
            ),
          ],
        ),
      ),
    );
  }
}
