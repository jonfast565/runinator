import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/api/command_center_api.dart' as api;
import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/app_service.dart';
import '../../core/services/org_admin_service.dart';
import '../../core/services/orgs_service.dart';
import '../../core/services/permissions_service.dart';
import '../shared/cc_widgets.dart';
import '../shared/confirm.dart';
import '../theme/app_theme.dart';

class OrganizationView extends ConsumerStatefulWidget {
  const OrganizationView({super.key});

  @override
  ConsumerState<OrganizationView> createState() => _OrganizationViewState();
}

class _OrganizationViewState extends ConsumerState<OrganizationView> {
  final _newOrgController = TextEditingController();
  final _newMemberController = TextEditingController();
  final _newTeamController = TextEditingController();
  final _teamMemberController = TextEditingController();

  List<api.OrgMembership> _members = const [];
  List<Team> _teams = const [];
  List<User> _teamMembers = const [];
  String? _selectedTeamId;
  var _newMemberRole = api.OrgRole.member;
  var _loadingMembers = false;

  @override
  void dispose() {
    _newOrgController.dispose();
    _newMemberController.dispose();
    _newTeamController.dispose();
    _teamMemberController.dispose();
    super.dispose();
  }

  Future<void> _loadMembers() async {
    final orgId = ref.read(orgsProvider).activeOrgId;
    if (orgId == null) {
      setState(() => _members = const []);
      return;
    }

    setState(() => _loadingMembers = true);
    try {
      final members = await ref.read(orgAdminServiceProvider).listMembers(orgId);
      if (mounted) setState(() => _members = members);
    } finally {
      if (mounted) setState(() => _loadingMembers = false);
    }
  }

  Future<void> _loadTeams() async {
    try {
      final teams = await ref.read(orgAdminServiceProvider).listTeams();
      if (mounted) setState(() => _teams = teams);
    } catch (_) {
      if (mounted) setState(() => _teams = const []);
    }
  }

  Future<void> _loadTeamMembers(String? teamId) async {
    if (teamId == null) {
      setState(() => _teamMembers = const []);
      return;
    }

    try {
      final members = await ref.read(orgAdminServiceProvider).listTeamMembers(teamId);
      if (mounted) setState(() => _teamMembers = members);
    } catch (_) {
      if (mounted) setState(() => _teamMembers = const []);
    }
  }

  String _userLabel(String userId) {
    for (final user in ref.read(permissionsProvider).users) {
      if (user.id == userId) return user.username;
    }
    return userId;
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      ref.read(permissionsProvider.notifier).refreshAll().catchError((_) {});
      _loadMembers();
      _loadTeams();
    });
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(orgsProvider.select((s) => s.activeOrgId), (prev, next) {
      if (prev != next) {
        _loadMembers();
        _loadTeams();
        setState(() => _selectedTeamId = null);
        _loadTeamMembers(null);
      }
    });

    final orgs = ref.watch(orgsProvider);
    final orgNotifier = ref.read(orgsProvider.notifier);
    final admin = orgNotifier.isActiveOrgAdmin();
    final activeOrg = orgNotifier.activeOrg();

    return Padding(
      padding: const EdgeInsets.all(12),
      child: ListView(
        children: [
          PanelCard(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                PanelToolbar(
                  title: 'Organizations',
                  actions: [
                    CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => orgNotifier.refresh()),
                  ],
                ),
                DropdownButtonFormField<String>(
                  decoration: const InputDecoration(labelText: 'Active organization'),
                  value: orgs.activeOrgId,
                  items: [
                    for (final membership in orgs.memberships)
                      DropdownMenuItem(value: membership.org.id, child: Text('${membership.org.name} (${membership.role.name})')),
                  ],
                  onChanged: (value) {
                    if (value != null) orgNotifier.setActive(value);
                  },
                ),
                if (activeOrg != null) ...[
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 8,
                    children: [
                      Chip(label: Text('org=${activeOrg.slug}', style: const TextStyle(fontSize: 11))),
                      Chip(label: Text('you are ${orgNotifier.activeRole()?.name ?? ''}', style: const TextStyle(fontSize: 11))),
                    ],
                  ),
                ],
                const SizedBox(height: 16),
                const Text('Create organization', style: TextStyle(fontWeight: FontWeight.w700)),
                const SizedBox(height: 8),
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _newOrgController,
                        decoration: const InputDecoration(hintText: 'Acme Inc.'),
                      ),
                    ),
                    const SizedBox(width: 8),
                    CcButton(
                      icon: IconName.plus,
                      label: 'Create',
                      variant: CcButtonVariant.primary,
                      dense: true,
                      onPressed: _newOrgController.text.trim().isEmpty
                          ? null
                          : () async {
                              await orgNotifier.create(_newOrgController.text.trim());
                              _newOrgController.clear();
                              await _loadMembers();
                            },
                    ),
                  ],
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          if (activeOrg == null)
            const PanelCard(child: EmptyState(message: 'Create or select an organization to manage its members.'))
          else ...[
            PanelCard(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  PanelToolbar(title: 'Members — ${activeOrg.name}'),
                  if (_loadingMembers) const LinearProgressIndicator(minHeight: 2),
                  if (_members.isEmpty)
                    const EmptyState(message: 'No members loaded.')
                  else
                    for (final member in _members)
                      ListTile(
                        title: Text(_userLabel(member.userId)),
                        subtitle: Text(member.role.name),
                        trailing: admin
                            ? Row(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  DropdownButton<api.OrgRole>(
                                    value: member.role,
                                    items: [
                                      for (final role in api.OrgRole.values)
                                        DropdownMenuItem(value: role, child: Text(role.name)),
                                    ],
                                    onChanged: (role) async {
                                      if (role == null || role == member.role) return;
                                      await ref.read(orgAdminServiceProvider).updateMember(activeOrg.id, member.userId, role);
                                      await _loadMembers();
                                    },
                                  ),
                                  IconButton(
                                    icon: const CcIcon(IconName.trash, size: 16),
                                    onPressed: () async {
                                      final confirm = FlutterConfirmContext(context);
                                      if (!await confirm.confirmAsync('Remove member ${_userLabel(member.userId)}?')) return;
                                      await ref.read(orgAdminServiceProvider).removeMember(activeOrg.id, member.userId);
                                      await _loadMembers();
                                    },
                                  ),
                                ],
                              )
                            : StatusBadge(member.role.name),
                      ),
                  if (admin) ...[
                    const SizedBox(height: 12),
                    Row(
                      children: [
                        Expanded(child: TextField(controller: _newMemberController, decoration: const InputDecoration(labelText: 'User id (uuid)'))),
                        const SizedBox(width: 8),
                        DropdownButton<api.OrgRole>(
                          value: _newMemberRole,
                          items: [for (final role in api.OrgRole.values) DropdownMenuItem(value: role, child: Text(role.name))],
                          onChanged: (value) {
                            if (value != null) setState(() => _newMemberRole = value);
                          },
                        ),
                        const SizedBox(width: 8),
                        CcButton(
                          label: 'Add member',
                          dense: true,
                          onPressed: _newMemberController.text.trim().isEmpty
                              ? null
                              : () async {
                                  await ref.read(orgAdminServiceProvider).addMember(activeOrg.id, _newMemberController.text.trim(), _newMemberRole);
                                  _newMemberController.clear();
                                  await _loadMembers();
                                },
                        ),
                      ],
                    ),
                  ],
                ],
              ),
            ),
            const SizedBox(height: 12),
            PanelCard(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  PanelToolbar(title: 'Teams'),
                  Text(
                    'Teams are named principals you can grant workflow access to.',
                    style: TextStyle(fontSize: 12, color: AppColors.textMuted),
                  ),
                  const SizedBox(height: 8),
                  Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(
                        child: Column(
                          children: [
                            for (final team in _teams)
                              ListTile(
                                selected: team.id == _selectedTeamId,
                                title: Text(team.name),
                                trailing: IconButton(
                                  icon: const CcIcon(IconName.trash, size: 16),
                                  onPressed: () async {
                                    final confirm = FlutterConfirmContext(context);
                                    if (!await confirm.confirmAsync('Delete team "${team.name}"?')) return;
                                    if (team.id != null) {
                                      await ref.read(orgAdminServiceProvider).deleteTeam(team.id!);
                                      if (_selectedTeamId == team.id) {
                                        setState(() => _selectedTeamId = null);
                                        _loadTeamMembers(null);
                                      }
                                      await _loadTeams();
                                    }
                                  },
                                ),
                                onTap: () {
                                  setState(() => _selectedTeamId = team.id);
                                  _loadTeamMembers(team.id);
                                },
                              ),
                            Row(
                              children: [
                                Expanded(child: TextField(controller: _newTeamController, decoration: const InputDecoration(labelText: 'New team'))),
                                CcButton(
                                  icon: IconName.plus,
                                  label: 'Create',
                                  dense: true,
                                  onPressed: _newTeamController.text.trim().isEmpty
                                      ? null
                                      : () async {
                                          await ref.read(orgAdminServiceProvider).createTeam(_newTeamController.text.trim());
                                          _newTeamController.clear();
                                          await _loadTeams();
                                        },
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Text(_selectedTeamId == null ? 'Select a team' : 'Team members', style: const TextStyle(fontWeight: FontWeight.w600)),
                            const SizedBox(height: 8),
                            for (final user in _teamMembers)
                              ListTile(
                                dense: true,
                                title: Text(user.username),
                                trailing: IconButton(
                                  icon: const Icon(Icons.close, size: 16),
                                  onPressed: () async {
                                    if (_selectedTeamId != null && user.id != null) {
                                      await ref.read(orgAdminServiceProvider).removeTeamMember(_selectedTeamId!, user.id!);
                                      await _loadTeamMembers(_selectedTeamId);
                                    }
                                  },
                                ),
                              ),
                            if (_selectedTeamId != null)
                              Row(
                                children: [
                                  Expanded(child: TextField(controller: _teamMemberController, decoration: const InputDecoration(labelText: 'User id'))),
                                  CcButton(
                                    label: 'Add',
                                    dense: true,
                                    onPressed: _teamMemberController.text.trim().isEmpty
                                        ? null
                                        : () async {
                                            await ref.read(orgAdminServiceProvider).addTeamMember(_selectedTeamId!, _teamMemberController.text.trim());
                                            _teamMemberController.clear();
                                            await _loadTeamMembers(_selectedTeamId);
                                          },
                                  ),
                                ],
                              ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }
}
