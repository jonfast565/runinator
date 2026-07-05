import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/services/app_service.dart';
import '../../core/services/resources_service.dart';
import '../../core/utils/approvals.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/confirm.dart';
import '../shared/code_editor.dart';
import '../shared/split_pane.dart';

class ResourcesView extends ConsumerWidget {
  const ResourcesView({super.key, required this.endpoint, required this.title});

  final String endpoint;
  final String title;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final resources = ref.watch(resourcesProvider);
    final notifier = ref.read(resourcesProvider.notifier);
    final records = resources.resourceRecords.where((record) {
      if (resources.selectedResourceEndpoint != endpoint) return false;
      if (endpoint == 'approvals' && resources.hideResolved && isResolved(record)) return false;
      final query = ref.read(appProvider.notifier).normalizedSearch;
      if (query.isEmpty) return true;
      return [record['id'], record['status'], record['message'], record['type']]
          .any((value) => displayValue(value).toLowerCase().contains(query));
    }).toList();
    final selected = resources.selectedResourceRecord;
    final selectedIndex = selected == null ? null : records.indexWhere((r) => r['id'] == selected['id']);
    final canDelete = endpoint == 'automation_events' && selected != null;

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        storageKey: 'command-center.resources.$endpoint.split',
        initialFirstFraction: 0.58,
        mobileShowSecond: selected != null,
        mobileBackTitle: title,
        onMobileBack: () => notifier.setSelectedResourceRecord(null),
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: title,
                actions: [
                  CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.refreshResourcesFor(endpoint)),
                  if (endpoint == 'approvals')
                    CcButton(
                      icon: IconName.check,
                      label: resources.hideResolved ? 'Show resolved' : 'Hide resolved',
                      dense: true,
                      onPressed: () => notifier.setHideResolved(!resources.hideResolved),
                    ),
                  if (canDelete)
                    CcButton(
                      icon: IconName.trash,
                      label: 'Delete',
                      variant: CcButtonVariant.danger,
                      dense: true,
                      onPressed: () async {
                        final confirm = FlutterConfirmContext(context);
                        if (!await confirm.confirmAsync('Delete this event record?')) return;
                        await notifier.deleteSelectedEvent();
                      },
                    ),
                ],
              ),
              Expanded(
                child: CcDataTable(
                  columns: const ['Type', 'Status', 'Summary'],
                  rows: [
                    for (final record in records)
                      [
                        notifier.recordType(record),
                        displayValue(record['status']),
                        notifier.recordSummary(record),
                      ],
                  ],
                  selectedIndex: selectedIndex != null && selectedIndex >= 0 ? selectedIndex : null,
                  onSelect: (index) => notifier.setSelectedResourceRecord(records[index]),
                  emptyMessage: 'No records.',
                ),
              ),
            ],
          ),
        ),
        second: selected == null
            ? EmptyState(message: 'Select a $title record.')
            : PanelCard(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Row(
                      children: [
                        StatusBadge(displayValue(selected['status'])),
                        const SizedBox(width: 8),
                        Expanded(child: Text(notifier.recordSummary(selected), style: const TextStyle(fontWeight: FontWeight.w600))),
                      ],
                    ),
                    if (endpoint == 'approvals' && !isResolved(selected))
                      Padding(
                        padding: const EdgeInsets.only(top: 8),
                        child: Wrap(
                          spacing: 8,
                          children: [
                            CcButton(icon: IconName.approve, label: 'Approve', variant: CcButtonVariant.primary, dense: true, onPressed: () => notifier.resolveApproval(ApprovalAction.approve)),
                            CcButton(icon: IconName.reject, label: 'Reject', variant: CcButtonVariant.danger, dense: true, onPressed: () => notifier.resolveApproval(ApprovalAction.reject)),
                          ],
                        ),
                      ),
                    Expanded(child: JsonEditor(value: selected.toString(), onChanged: (_) {}, readOnly: true)),
                  ],
                ),
              ),
      ),
    );
  }
}

class ApprovalsView extends ConsumerWidget {
  const ApprovalsView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => const ResourcesView(endpoint: 'approvals', title: 'Approvals');
}

class ExternalItemsView extends ConsumerWidget {
  const ExternalItemsView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => const ResourcesView(endpoint: 'external_items', title: 'External Items');
}

class EventsView extends ConsumerWidget {
  const EventsView({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => const ResourcesView(endpoint: 'automation_events', title: 'Events');
}
