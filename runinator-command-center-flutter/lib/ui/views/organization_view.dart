import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/app_service.dart';
import '../../core/services/orgs_service.dart';
import '../shared/cc_widgets.dart';

class OrganizationView extends ConsumerWidget {
  const OrganizationView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final orgs = ref.watch(orgsProvider);
    final notifier = ref.read(orgsProvider.notifier);
    final app = ref.watch(appProvider);

    return Padding(
      padding: const EdgeInsets.all(12),
      child: PanelCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PanelToolbar(
              title: 'Organization',
              actions: [
                CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.refresh()),
              ],
            ),
            if (orgs.memberships.isEmpty)
              const Expanded(child: EmptyState(message: 'No organizations.'))
            else
              Expanded(
                child: ListView(
                  children: [
                    for (final membership in orgs.memberships)
                      ListTile(
                        selected: membership.org.id == orgs.activeOrgId,
                        title: Text(membership.org.name),
                        subtitle: Text(membership.role.name),
                        trailing: membership.org.id == orgs.activeOrgId ? const StatusBadge('active') : null,
                        onTap: () => notifier.setActive(membership.org.id),
                      ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}
