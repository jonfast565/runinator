import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/navigation/app_tab.dart';
import '../../core/services/app_service.dart';
import '../../core/services/orgs_service.dart';
import '../../core/services/workflow_sharing_service.dart';
import '../../core/services/workflows_service.dart';
import '../shared/cc_widgets.dart';
import '../shared/run_input_form.dart';

class RunInputModal extends ConsumerWidget {
  const RunInputModal({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final workflows = ref.watch(workflowsProvider);
    if (!workflows.runInputOpen) return const SizedBox.shrink();

    final notifier = ref.read(workflowsProvider.notifier);
    final host = notifier.host;
    final inputType = host.getSelectedWorkflowInputType();

    return _ModalShell(
      title: workflows.runInputDebug ? 'Debug Run' : 'Run Workflow',
      onClose: () => notifier.runs.closeRunInput(),
      actions: [
        CcButton(label: 'Cancel', onPressed: () => notifier.runs.closeRunInput()),
        CcButton(
          label: workflows.runInputDebug ? 'Debug Run' : 'Run',
          variant: CcButtonVariant.primary,
          onPressed: () async {
            await notifier.runs.confirmRunInput();
            ref.read(appProvider.notifier).setActiveTab(AppTab.runs);
          },
        ),
      ],
      child: RunInputForm(
        inputType: inputType,
        draft: Map<String, Object?>.from(workflows.runInputDraft),
        onChanged: (next) {
          host.state.runInputDraft = Map<String, Object?>.from(next);
          host.notify();
        },
      ),
    );
  }
}

class WorkflowSettingsModal extends ConsumerStatefulWidget {
  const WorkflowSettingsModal({super.key});

  @override
  ConsumerState<WorkflowSettingsModal> createState() => _WorkflowSettingsModalState();
}

class _WorkflowSettingsModalState extends ConsumerState<WorkflowSettingsModal> {
  late final TextEditingController _nameController;
  late final TextEditingController _versionController;
  late final TextEditingController _concurrencyController;

  @override
  void initState() {
    super.initState();
    final workflows = ref.read(workflowsProvider);
    _nameController = TextEditingController(text: workflows.workflowDraft.name);
    _versionController = TextEditingController(text: workflows.workflowDraft.version);
    _concurrencyController = TextEditingController(text: workflows.workflowConcurrency.toString());
  }

  @override
  void dispose() {
    _nameController.dispose();
    _versionController.dispose();
    _concurrencyController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final workflows = ref.watch(workflowsProvider);
    if (!workflows.workflowSettingsOpen) return const SizedBox.shrink();

    final notifier = ref.read(workflowsProvider.notifier);
    final host = notifier.host;
    final orgs = ref.watch(orgsProvider);
    final draft = workflows.workflowDraft;

    return _ModalShell(
      title: 'Workflow Settings',
      onClose: () => notifier.catalog.closeWorkflowSettings(),
      actions: [
        CcButton(label: 'Done', variant: CcButtonVariant.primary, onPressed: () => notifier.catalog.closeWorkflowSettings()),
      ],
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              decoration: const InputDecoration(labelText: 'Name'),
              controller: _nameController,
              onChanged: (v) {
                draft.name = v;
                host.notify();
              },
            ),
            const SizedBox(height: 8),
            TextField(
              decoration: const InputDecoration(labelText: 'Version'),
              controller: _versionController,
              onChanged: (v) {
                draft.version = v;
                host.notify();
              },
            ),
            const SizedBox(height: 8),
            SwitchListTile(
              title: const Text('Enabled'),
              value: draft.enabled,
              onChanged: (v) {
                draft.enabled = v;
                host.notify();
              },
            ),
            TextField(
              decoration: const InputDecoration(labelText: 'Concurrency'),
              keyboardType: TextInputType.number,
              controller: _concurrencyController,
              onChanged: (v) {
                workflows.workflowConcurrency = num.tryParse(v) ?? workflows.workflowConcurrency;
                host.notify();
              },
            ),
            const SizedBox(height: 16),
            PanelToolbar(
              title: 'Triggers',
              actions: [
                CcButton(icon: IconName.plus, label: 'Add', dense: true, onPressed: () => notifier.catalog.addWorkflowTrigger()),
                CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => notifier.catalog.refreshWorkflowTriggers()),
              ],
            ),
            for (final trigger in workflows.workflowTriggers)
              ListTile(
                title: Text(trigger.kind.wire),
                subtitle: Text(trigger.enabled ? 'enabled' : 'disabled'),
                trailing: IconButton(
                  icon: const Icon(Icons.edit, size: 16),
                  onPressed: () => notifier.catalog.editWorkflowTrigger(trigger),
                ),
              ),
            if (orgs.memberships.isNotEmpty) ...[
              const SizedBox(height: 16),
              const Text('Ownership', style: TextStyle(fontWeight: FontWeight.w700)),
              DropdownButton<String?>(
                isExpanded: true,
                value: draft.orgId,
                items: [
                  const DropdownMenuItem(value: null, child: Text('Global')),
                  for (final m in orgs.memberships)
                    DropdownMenuItem(value: m.org.id, child: Text(m.org.name)),
                ],
                onChanged: draft.id == null
                    ? null
                    : (orgId) async {
                        await ref.read(workflowSharingServiceProvider).setOwner(draft.id!, orgId);
                        draft.orgId = orgId;
                        host.notify();
                      },
              ),
            ],
            const SizedBox(height: 16),
            Wrap(
              spacing: 8,
              children: [
                CcButton(
                  icon: IconName.trash,
                  label: 'Delete',
                  variant: CcButtonVariant.danger,
                  onPressed: () => notifier.catalog.deleteSelectedWorkflow(),
                ),
                CcButton(icon: IconName.edit, label: 'Duplicate', onPressed: () => notifier.catalog.duplicateSelectedWorkflow()),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class ShareWorkflowModal extends ConsumerStatefulWidget {
  const ShareWorkflowModal({super.key, required this.workflowId, required this.onClose});

  final String workflowId;
  final VoidCallback onClose;

  @override
  ConsumerState<ShareWorkflowModal> createState() => _ShareWorkflowModalState();
}

class _ShareWorkflowModalState extends ConsumerState<ShareWorkflowModal> {
  List<Grant> _grants = const [];
  PrincipalType _principalType = PrincipalType.user;
  String _principalId = '';
  PermissionLevel _permission = PermissionLevel.view;
  var _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final records = await ref.read(workflowSharingServiceProvider).listGrants(widget.workflowId);
      setState(() => _grants = records.map(Grant.fromJson).toList());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return _ModalShell(
      title: 'Share Workflow',
      onClose: widget.onClose,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (_loading) const LinearProgressIndicator(minHeight: 2),
          Expanded(
            child: ListView(
              children: [
                for (final grant in _grants)
                  ListTile(
                    title: Text('${grant.principalType.wire} · ${grant.principalId}'),
                    subtitle: Text(grant.permission.wire),
                    trailing: grant.id == null
                        ? null
                        : IconButton(
                            icon: const Icon(Icons.delete_outline, size: 16),
                            onPressed: () async {
                              await ref.read(workflowSharingServiceProvider).revokeGrant(widget.workflowId, grant.id!);
                              await _load();
                            },
                          ),
                  ),
                const Divider(),
                DropdownButton<PrincipalType>(
                  isExpanded: true,
                  value: _principalType,
                  items: PrincipalType.values.map((t) => DropdownMenuItem(value: t, child: Text(t.wire))).toList(),
                  onChanged: (v) => setState(() => _principalType = v ?? _principalType),
                ),
                TextField(
                  decoration: const InputDecoration(labelText: 'Principal ID'),
                  onChanged: (v) => _principalId = v,
                ),
                DropdownButton<PermissionLevel>(
                  isExpanded: true,
                  value: _permission,
                  items: PermissionLevel.values.map((p) => DropdownMenuItem(value: p, child: Text(p.wire))).toList(),
                  onChanged: (v) => setState(() => _permission = v ?? _permission),
                ),
                CcButton(
                  label: 'Grant access',
                  variant: CcButtonVariant.primary,
                  onPressed: _principalId.isEmpty
                      ? null
                      : () async {
                          await ref.read(workflowSharingServiceProvider).createGrant(
                                widget.workflowId,
                                _principalType,
                                _principalId,
                                _permission,
                              );
                          await _load();
                        },
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _ModalShell extends StatelessWidget {
  const _ModalShell({required this.title, required this.onClose, required this.child, this.actions = const []});

  final String title;
  final VoidCallback onClose;
  final Widget child;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.black54,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720, maxHeight: 640),
          child: Card(
            margin: const EdgeInsets.all(24),
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Expanded(child: Text(title, style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 16))),
                      IconButton(icon: const Icon(Icons.close), onPressed: onClose),
                    ],
                  ),
                  Expanded(child: child),
                  if (actions.isNotEmpty) ...[
                    const SizedBox(height: 12),
                    Wrap(spacing: 8, children: actions),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
