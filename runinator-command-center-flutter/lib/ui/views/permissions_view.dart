import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/app_service.dart';
import '../../core/services/permissions_service.dart';
import '../../core/services/workflows_service.dart';
import '../../core/utils/values.dart';
import '../shared/cc_widgets.dart';
import '../shared/confirm.dart';
import '../shared/split_pane.dart';
import '../theme/app_theme.dart';

class PermissionsView extends ConsumerStatefulWidget {
  const PermissionsView({super.key});

  @override
  ConsumerState<PermissionsView> createState() => _PermissionsViewState();
}

class _PermissionsViewState extends ConsumerState<PermissionsView> {
  var _tab = 0;
  UserDraft? _userDraft;
  ApiKeyDraft? _apiKeyDraft;
  var _teamDraftName = '';
  GrantDraft? _grantDraft;

  // each tab has its own list<->detail split, and the detail pane doubles as a
  // "create new" form, so "is something selected" alone isn't enough to know
  // whether mobile should show the detail pane — track it explicitly per tab.
  var _usersMobileDetail = false;
  var _teamsMobileDetail = false;
  var _accessMobileDetail = false;
  var _apiKeysMobileDetail = false;

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(permissionsProvider);
    final notifier = ref.read(permissionsProvider.notifier);
    final workflows = ref.watch(workflowsProvider);
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
                0 => _UsersTab(
                    state: state,
                    notifier: notifier,
                    query: query,
                    draft: _userDraft,
                    onDraft: (draft) => setState(() => _userDraft = draft),
                    mobileDetail: _usersMobileDetail,
                    onMobileDetailChanged: (open) => setState(() => _usersMobileDetail = open),
                  ),
                1 => _TeamsTab(
                    state: state,
                    notifier: notifier,
                    query: query,
                    teamDraftName: _teamDraftName,
                    onTeamDraft: (name) => setState(() => _teamDraftName = name),
                    mobileDetail: _teamsMobileDetail,
                    onMobileDetailChanged: (open) => setState(() => _teamsMobileDetail = open),
                  ),
                2 => _AccessTab(
                    state: state,
                    notifier: notifier,
                    workflows: workflows.workflows,
                    query: query,
                    grantDraft: _grantDraft,
                    onGrantDraft: (draft) => setState(() => _grantDraft = draft),
                    mobileDetail: _accessMobileDetail,
                    onMobileDetailChanged: (open) => setState(() => _accessMobileDetail = open),
                  ),
                _ => _ApiKeysTab(
                    state: state,
                    notifier: notifier,
                    query: query,
                    draft: _apiKeyDraft,
                    onDraft: (draft) => setState(() => _apiKeyDraft = draft),
                    mobileDetail: _apiKeysMobileDetail,
                    onMobileDetailChanged: (open) => setState(() => _apiKeysMobileDetail = open),
                  ),
              },
            ),
          ],
        ),
      ),
    );
  }
}

class _UsersTab extends StatelessWidget {
  const _UsersTab({
    required this.state,
    required this.notifier,
    required this.query,
    required this.draft,
    required this.onDraft,
    required this.mobileDetail,
    required this.onMobileDetailChanged,
  });

  final PermissionsState state;
  final PermissionsNotifier notifier;
  final String query;
  final UserDraft? draft;
  final ValueChanged<UserDraft?> onDraft;
  final bool mobileDetail;
  final ValueChanged<bool> onMobileDetailChanged;

  @override
  Widget build(BuildContext context) {
    final users = notifier.filteredUsers(query);
    final selected = notifier.selectedUser();
    final userDraft = draft ?? (selected != null ? userDraftFrom(selected) : blankUserDraft());

    return SplitPane(
      storageKey: 'command-center.permissions.users.split',
      initialFirstFraction: 0.55,
      mobileShowSecond: mobileDetail,
      mobileBackTitle: selected == null ? 'New user' : selected.username,
      onMobileBack: () => onMobileDetailChanged(false),
      first: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          PanelToolbar(
            title: 'Users (${users.length})',
            actions: [
              CcButton(icon: IconName.plus, label: 'New User', variant: CcButtonVariant.primary, dense: true, onPressed: () {
                onDraft(notifier.clearUserSelection());
                onMobileDetailChanged(true);
              }),
            ],
          ),
          Expanded(
            child: CcDataTable(
              columns: const ['Username', 'Email', 'Status', 'Role'],
              rows: [
                for (final user in users)
                  [
                    user.username,
                    user.email ?? '-',
                    user.disabled ? 'disabled' : 'active',
                    user.isAdmin ? 'admin' : 'user',
                  ],
              ],
              selectedIndex: selected == null ? null : users.indexWhere((u) => u.id == selected.id),
              onSelect: (index) {
                onDraft(notifier.selectUser(users[index]));
                onMobileDetailChanged(true);
              },
              emptyMessage: 'No users.',
            ),
          ),
        ],
      ),
      second: _UserEditor(
        draft: userDraft,
        editing: selected != null,
        onChanged: onDraft,
        onSave: () => notifier.saveUserDraft(userDraft),
        onDelete: selected == null
            ? null
            : () async {
                final confirm = FlutterConfirmContext(context);
                if (!await confirm.confirmAsync('Delete user "${selected.username}"?')) return;
                await notifier.deleteSelectedUser();
                onDraft(null);
                onMobileDetailChanged(false);
              },
      ),
    );
  }
}

class _UserEditor extends StatelessWidget {
  const _UserEditor({required this.draft, required this.editing, required this.onChanged, required this.onSave, this.onDelete});

  final UserDraft draft;
  final bool editing;
  final ValueChanged<UserDraft?> onChanged;
  final VoidCallback onSave;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(12),
      children: [
        Text(editing ? 'Edit user' : 'New user', style: const TextStyle(fontWeight: FontWeight.w700)),
        const SizedBox(height: 12),
        TextField(
          decoration: InputDecoration(labelText: editing ? 'Username (read-only)' : 'Username'),
          readOnly: editing,
          controller: TextEditingController(text: draft.username),
          onChanged: editing ? null : (v) => onChanged(UserDraft(username: v, email: draft.email, password: draft.password, isAdmin: draft.isAdmin, disabled: draft.disabled)),
        ),
        TextField(
          decoration: const InputDecoration(labelText: 'Email'),
          controller: TextEditingController(text: draft.email),
          onChanged: (v) => onChanged(UserDraft(username: draft.username, email: v, password: draft.password, isAdmin: draft.isAdmin, disabled: draft.disabled)),
        ),
        TextField(
          decoration: InputDecoration(labelText: editing ? 'Password (leave blank to keep)' : 'Password'),
          obscureText: true,
          controller: TextEditingController(text: draft.password),
          onChanged: (v) => onChanged(UserDraft(username: draft.username, email: draft.email, password: v, isAdmin: draft.isAdmin, disabled: draft.disabled)),
        ),
        SwitchListTile(
          title: const Text('Admin'),
          value: draft.isAdmin,
          onChanged: (v) => onChanged(UserDraft(username: draft.username, email: draft.email, password: draft.password, isAdmin: v, disabled: draft.disabled)),
        ),
        if (editing)
          SwitchListTile(
            title: const Text('Disabled'),
            value: draft.disabled,
            onChanged: (v) => onChanged(UserDraft(username: draft.username, email: draft.email, password: draft.password, isAdmin: draft.isAdmin, disabled: v)),
          ),
        const SizedBox(height: 12),
        Wrap(
          spacing: 8,
          children: [
            CcButton(icon: IconName.save, label: 'Save', variant: CcButtonVariant.primary, dense: true, onPressed: onSave),
            if (onDelete != null) CcButton(icon: IconName.trash, label: 'Delete', variant: CcButtonVariant.danger, dense: true, onPressed: onDelete),
          ],
        ),
      ],
    );
  }
}

class _TeamsTab extends StatelessWidget {
  const _TeamsTab({
    required this.state,
    required this.notifier,
    required this.query,
    required this.teamDraftName,
    required this.onTeamDraft,
    required this.mobileDetail,
    required this.onMobileDetailChanged,
  });

  final PermissionsState state;
  final PermissionsNotifier notifier;
  final String query;
  final String teamDraftName;
  final ValueChanged<String> onTeamDraft;
  final bool mobileDetail;
  final ValueChanged<bool> onMobileDetailChanged;

  @override
  Widget build(BuildContext context) {
    final teams = notifier.filteredTeams(query);
    final selected = notifier.selectedTeam();

    return SplitPane(
      storageKey: 'command-center.permissions.teams.split',
      initialFirstFraction: 0.45,
      mobileShowSecond: mobileDetail,
      mobileBackTitle: selected == null ? 'New team' : selected.name,
      onMobileBack: () => onMobileDetailChanged(false),
      first: Column(
        children: [
          PanelToolbar(
            title: 'Teams (${teams.length})',
            actions: [
              CcButton(icon: IconName.plus, label: 'New Team', variant: CcButtonVariant.primary, dense: true, onPressed: () {
                onTeamDraft('');
                notifier.clearTeamSelection();
                onMobileDetailChanged(true);
              }),
            ],
          ),
          Expanded(
            child: CcDataTable(
              columns: const ['Name', 'Created'],
              rows: [for (final team in teams) [team.name, team.createdAt ?? '-']],
              selectedIndex: selected == null ? null : teams.indexWhere((t) => t.id == selected.id),
              onSelect: (index) {
                onTeamDraft(notifier.selectTeam(teams[index]));
                onMobileDetailChanged(true);
              },
              emptyMessage: 'No teams.',
            ),
          ),
        ],
      ),
      second: ListView(
        padding: const EdgeInsets.all(12),
        children: [
          Text(selected == null ? 'New team' : 'Edit team', style: const TextStyle(fontWeight: FontWeight.w700)),
          TextField(
            decoration: const InputDecoration(labelText: 'Team name'),
            controller: TextEditingController(text: teamDraftName.isEmpty ? (selected?.name ?? '') : teamDraftName),
            onChanged: onTeamDraft,
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            children: [
              CcButton(
                icon: IconName.save,
                label: 'Save',
                variant: CcButtonVariant.primary,
                dense: true,
                onPressed: () => notifier.saveTeamDraft(teamDraftName.isEmpty ? (selected?.name ?? '') : teamDraftName),
              ),
              if (selected != null)
                CcButton(
                  icon: IconName.trash,
                  label: 'Delete',
                  variant: CcButtonVariant.danger,
                  dense: true,
                  onPressed: () async {
                    final confirm = FlutterConfirmContext(context);
                    if (!await confirm.confirmAsync('Delete team "${selected.name}"?')) return;
                    await notifier.deleteSelectedTeam();
                    onTeamDraft('');
                    onMobileDetailChanged(false);
                  },
                ),
            ],
          ),
          if (selected != null) ...[
            const SizedBox(height: 16),
            Text('Members (${state.teamMembers.length})', style: const TextStyle(fontWeight: FontWeight.w600)),
            for (final member in state.teamMembers)
              ListTile(
                dense: true,
                title: Text(member.username),
                trailing: IconButton(
                  icon: const Icon(Icons.close, size: 16),
                  onPressed: () => notifier.removeSelectedTeamMember(member.id ?? ''),
                ),
              ),
          ],
        ],
      ),
    );
  }
}

class _AccessTab extends StatelessWidget {
  const _AccessTab({
    required this.state,
    required this.notifier,
    required this.workflows,
    required this.query,
    required this.grantDraft,
    required this.onGrantDraft,
    required this.mobileDetail,
    required this.onMobileDetailChanged,
  });

  final PermissionsState state;
  final PermissionsNotifier notifier;
  final List<WorkflowDefinition> workflows;
  final String query;
  final GrantDraft? grantDraft;
  final ValueChanged<GrantDraft?> onGrantDraft;
  final bool mobileDetail;
  final ValueChanged<bool> onMobileDetailChanged;

  @override
  Widget build(BuildContext context) {
    final filtered = workflows.where((w) {
      if (query.isEmpty) return true;
      return [w.name, w.id, w.version?.toString()].any((v) => displayValue(v).toLowerCase().contains(query));
    }).toList();
    final grant = grantDraft ?? blankGrantDraft();
    final selectedWorkflow = state.selectedWorkflowId == null
        ? null
        : filtered.cast<WorkflowDefinition?>().firstWhere((w) => w?.id == state.selectedWorkflowId, orElse: () => null);

    return SplitPane(
      storageKey: 'command-center.permissions.access.split',
      initialFirstFraction: 0.4,
      mobileShowSecond: mobileDetail && state.selectedWorkflowId != null,
      mobileBackTitle: selectedWorkflow?.name ?? selectedWorkflow?.id ?? 'Grants',
      onMobileBack: () => onMobileDetailChanged(false),
      first: Column(
        children: [
          const PanelToolbar(title: 'Workflows'),
          Expanded(
            child: CcDataTable(
              columns: const ['Name', 'Version'],
              rows: [for (final wf in filtered) [wf.name ?? wf.id ?? '-', wf.version?.toString() ?? '-']],
              selectedIndex: state.selectedWorkflowId == null ? null : filtered.indexWhere((w) => w.id == state.selectedWorkflowId),
              onSelect: (index) async {
                onGrantDraft(await notifier.selectWorkflow(filtered[index].id));
                onMobileDetailChanged(true);
              },
              emptyMessage: 'No workflows.',
            ),
          ),
        ],
      ),
      second: ListView(
        padding: const EdgeInsets.all(12),
        children: [
          PanelToolbar(
            title: 'Grants',
            actions: [
              CcButton(
                icon: IconName.plus,
                label: 'Add Access',
                variant: CcButtonVariant.primary,
                dense: true,
                onPressed: state.selectedWorkflowId == null ? null : () => onGrantDraft(blankGrantDraft()),
              ),
            ],
          ),
          for (final entry in state.workflowGrants)
            ListTile(
              dense: true,
              title: Text('${entry.principalType.name}:${entry.principalId}'),
              subtitle: Text(entry.permission.name),
              trailing: IconButton(
                icon: const CcIcon(IconName.trash, size: 16),
                onPressed: () => notifier.revokeGrant(entry.id),
              ),
            ),
          if (state.selectedWorkflowId != null) ...[
            const Divider(),
            DropdownButtonFormField<PrincipalType>(
              decoration: const InputDecoration(labelText: 'Principal type'),
              value: grant.principalType,
              items: PrincipalType.values.map((t) => DropdownMenuItem(value: t, child: Text(t.name))).toList(),
              onChanged: (v) {
                if (v != null) onGrantDraft(GrantDraft(principalType: v, principalId: grant.principalId, permission: grant.permission));
              },
            ),
            TextField(
              decoration: const InputDecoration(labelText: 'Principal id'),
              controller: TextEditingController(text: grant.principalId),
              onChanged: (v) => onGrantDraft(GrantDraft(principalType: grant.principalType, principalId: v, permission: grant.permission)),
            ),
            DropdownButtonFormField<PermissionLevel>(
              decoration: const InputDecoration(labelText: 'Permission'),
              value: grant.permission,
              items: permissionLevels.map((p) => DropdownMenuItem(value: p, child: Text(p.name))).toList(),
              onChanged: (v) {
                if (v != null) onGrantDraft(GrantDraft(principalType: grant.principalType, principalId: grant.principalId, permission: v));
              },
            ),
            CcButton(
              icon: IconName.save,
              label: 'Save access',
              variant: CcButtonVariant.primary,
              dense: true,
              onPressed: () async {
                final next = await notifier.saveGrantDraft(grant);
                onGrantDraft(next);
              },
            ),
          ],
        ],
      ),
    );
  }
}

class _ApiKeysTab extends StatelessWidget {
  const _ApiKeysTab({
    required this.state,
    required this.notifier,
    required this.query,
    required this.draft,
    required this.onDraft,
    required this.mobileDetail,
    required this.onMobileDetailChanged,
  });

  final PermissionsState state;
  final PermissionsNotifier notifier;
  final String query;
  final ApiKeyDraft? draft;
  final ValueChanged<ApiKeyDraft?> onDraft;
  final bool mobileDetail;
  final ValueChanged<bool> onMobileDetailChanged;

  @override
  Widget build(BuildContext context) {
    final keys = notifier.visibleApiKeys(query);
    final selected = notifier.selectedApiKey();
    final keyDraft = draft ?? (selected != null ? apiKeyDraftFrom(selected) : blankApiKeyDraft(state.selectedUserId));

    return SplitPane(
      storageKey: 'command-center.permissions.apikeys.split',
      initialFirstFraction: 0.55,
      mobileShowSecond: mobileDetail,
      mobileBackTitle: selected == null ? 'New API key' : selected.name,
      onMobileBack: () => onMobileDetailChanged(false),
      first: Column(
        children: [
          PanelToolbar(
            title: 'API Keys (${keys.length})',
            actions: [
              CcButton(icon: IconName.plus, label: 'New Key', variant: CcButtonVariant.primary, dense: true, onPressed: () {
                onDraft(notifier.clearApiKeySelection(state.selectedUserId));
                onMobileDetailChanged(true);
              }),
            ],
          ),
          Expanded(
            child: CcDataTable(
              columns: const ['Name', 'Prefix', 'Status'],
              rows: [
                for (final key in keys)
                  [key.name, key.keyPrefix, key.disabled ? 'disabled' : 'active'],
              ],
              selectedIndex: selected == null ? null : keys.indexWhere((k) => k.id == selected.id),
              onSelect: (index) {
                onDraft(notifier.selectApiKey(keys[index], state.selectedUserId));
                onMobileDetailChanged(true);
              },
              emptyMessage: 'No API keys.',
            ),
          ),
        ],
      ),
      second: ListView(
        padding: const EdgeInsets.all(12),
        children: [
          Text(selected == null ? 'New API key' : 'Edit API key', style: const TextStyle(fontWeight: FontWeight.w700)),
          TextField(
            decoration: const InputDecoration(labelText: 'Name'),
            controller: TextEditingController(text: keyDraft.name),
            onChanged: (v) => onDraft(ApiKeyDraft(name: v, userId: keyDraft.userId, isService: keyDraft.isService, expiresAt: keyDraft.expiresAt, disabled: keyDraft.disabled)),
          ),
          SwitchListTile(
            title: const Text('Service key'),
            value: keyDraft.isService,
            onChanged: (v) => onDraft(ApiKeyDraft(name: keyDraft.name, userId: keyDraft.userId, isService: v, expiresAt: keyDraft.expiresAt, disabled: keyDraft.disabled)),
          ),
          if (selected != null)
            SwitchListTile(
              title: const Text('Disabled'),
              value: keyDraft.disabled,
              onChanged: (v) => onDraft(ApiKeyDraft(name: keyDraft.name, userId: keyDraft.userId, isService: keyDraft.isService, expiresAt: keyDraft.expiresAt, disabled: v)),
            ),
          if (state.revealedApiKey != null) ...[
            const SizedBox(height: 8),
            SelectableText('Key: ${state.revealedApiKey!.secret}', style: const TextStyle(fontFamily: kMonoFontFamily, fontFamilyFallback: kMonoFontFamilyFallback, fontSize: 12)),
          ],
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            children: [
              CcButton(icon: IconName.save, label: 'Save', variant: CcButtonVariant.primary, dense: true, onPressed: () => notifier.saveApiKeyDraft(keyDraft)),
              if (selected != null) ...[
                CcButton(icon: IconName.refresh, label: 'Rotate', dense: true, onPressed: () => notifier.rotateSelectedApiKey()),
                CcButton(icon: IconName.trash, label: 'Revoke', variant: CcButtonVariant.danger, dense: true, onPressed: () async {
                  final confirm = FlutterConfirmContext(context);
                  if (!await confirm.confirmAsync('Revoke API key "${selected.name}"?')) return;
                  await notifier.revokeSelectedApiKey();
                  onDraft(null);
                  onMobileDetailChanged(false);
                }),
              ],
            ],
          ),
        ],
      ),
    );
  }
}
