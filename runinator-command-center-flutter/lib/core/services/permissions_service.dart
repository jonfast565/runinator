// port of core/services/permissions.ts.

import 'dart:async' show unawaited;

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/models/index.dart';
import 'app_service.dart';

part 'permissions_service.g.dart';

final List<PermissionLevel> permissionLevels = [
  PermissionLevel.view,
  PermissionLevel.run,
  PermissionLevel.edit,
  PermissionLevel.own,
];

class UserDraft {
  const UserDraft({required this.username, required this.email, required this.password, required this.isAdmin, required this.disabled});

  final String username;
  final String email;
  final String password;
  final bool isAdmin;
  final bool disabled;
}

class GrantDraft {
  const GrantDraft({required this.principalType, required this.principalId, required this.permission});

  final PrincipalType principalType;
  final String principalId;
  final PermissionLevel permission;
}

class ApiKeyDraft {
  const ApiKeyDraft({
    required this.name,
    required this.userId,
    required this.isService,
    required this.expiresAt,
    required this.disabled,
  });

  final String name;
  final String userId;
  final bool isService;
  final String expiresAt;
  final bool disabled;
}

UserDraft blankUserDraft() => const UserDraft(username: '', email: '', password: '', isAdmin: false, disabled: false);

UserDraft userDraftFrom(User user) =>
    UserDraft(username: user.username, email: user.email ?? '', password: '', isAdmin: user.isAdmin, disabled: user.disabled);

ApiKeyDraft blankApiKeyDraft([String? userId]) =>
    ApiKeyDraft(name: '', userId: userId ?? '', isService: false, expiresAt: '', disabled: false);

ApiKeyDraft apiKeyDraftFrom(ApiKey apiKey) => ApiKeyDraft(
      name: apiKey.name,
      userId: apiKey.userId ?? '',
      isService: apiKey.isService,
      expiresAt: apiKey.expiresAt != null ? _toDateTimeInput(apiKey.expiresAt!) : '',
      disabled: apiKey.disabled,
    );

GrantDraft blankGrantDraft() => const GrantDraft(principalType: PrincipalType.user, principalId: '', permission: PermissionLevel.view);

class PermissionsState {
  const PermissionsState({
    required this.users,
    required this.teams,
    required this.apiKeys,
    this.selectedUserId,
    this.selectedTeamId,
    this.selectedApiKeyId,
    this.selectedWorkflowId,
    required this.workflowGrants,
    required this.teamMembers,
    required this.userTeams,
    this.revealedApiKey,
  });

  final List<User> users;
  final List<Team> teams;
  final List<ApiKey> apiKeys;
  final String? selectedUserId;
  final String? selectedTeamId;
  final String? selectedApiKeyId;
  final String? selectedWorkflowId;
  final List<Grant> workflowGrants;
  final List<User> teamMembers;
  final List<Team> userTeams;
  final CreateApiKeyResponse? revealedApiKey;

  PermissionsState copyWith({
    List<User>? users,
    List<Team>? teams,
    List<ApiKey>? apiKeys,
    Object? selectedUserId = _unset,
    Object? selectedTeamId = _unset,
    Object? selectedApiKeyId = _unset,
    Object? selectedWorkflowId = _unset,
    List<Grant>? workflowGrants,
    List<User>? teamMembers,
    List<Team>? userTeams,
    Object? revealedApiKey = _unset,
  }) =>
      PermissionsState(
        users: users ?? this.users,
        teams: teams ?? this.teams,
        apiKeys: apiKeys ?? this.apiKeys,
        selectedUserId: identical(selectedUserId, _unset) ? this.selectedUserId : selectedUserId as String?,
        selectedTeamId: identical(selectedTeamId, _unset) ? this.selectedTeamId : selectedTeamId as String?,
        selectedApiKeyId: identical(selectedApiKeyId, _unset) ? this.selectedApiKeyId : selectedApiKeyId as String?,
        selectedWorkflowId: identical(selectedWorkflowId, _unset) ? this.selectedWorkflowId : selectedWorkflowId as String?,
        workflowGrants: workflowGrants ?? this.workflowGrants,
        teamMembers: teamMembers ?? this.teamMembers,
        userTeams: userTeams ?? this.userTeams,
        revealedApiKey: identical(revealedApiKey, _unset) ? this.revealedApiKey : revealedApiKey as CreateApiKeyResponse?,
      );
}

const Object _unset = Object();

PermissionsState _initialPermissionsState() => const PermissionsState(
      users: [],
      teams: [],
      apiKeys: [],
      selectedUserId: null,
      selectedTeamId: null,
      selectedApiKeyId: null,
      selectedWorkflowId: null,
      workflowGrants: [],
      teamMembers: [],
      userTeams: [],
      revealedApiKey: null,
    );

@riverpod
class PermissionsNotifier extends _$PermissionsNotifier {
  @override
  PermissionsState build() => _initialPermissionsState();

  User? selectedUser() {
    for (final user in state.users) {
      if (user.id == state.selectedUserId) return user;
    }
    return null;
  }

  Team? selectedTeam() {
    for (final team in state.teams) {
      if (team.id == state.selectedTeamId) return team;
    }
    return null;
  }

  ApiKey? selectedApiKey() {
    for (final apiKey in state.apiKeys) {
      if (apiKey.id == state.selectedApiKeyId) return apiKey;
    }
    return null;
  }

  List<User> filteredUsers(String query) {
    if (query.isEmpty) return state.users;
    return state.users.where((user) => _userSearchText(user).contains(query)).toList();
  }

  List<Team> filteredTeams(String query) {
    if (query.isEmpty) return state.teams;
    return state.teams.where((team) => _teamSearchText(team).contains(query)).toList();
  }

  List<ApiKey> visibleApiKeys(String query) {
    final selectedUserId = state.selectedUserId;
    final visible = selectedUserId != null
        ? state.apiKeys.where((key) => key.isService || key.userId == selectedUserId).toList()
        : state.apiKeys;

    if (query.isEmpty) return visible;
    return visible.where((key) => _apiKeySearchText(key, state.users).contains(query)).toList();
  }

  int enabledAdminCount() => state.users.where((user) => user.isAdmin && !user.disabled).length;

  void clearRevealedApiKey() {
    state = state.copyWith(revealedApiKey: null);
  }

  Future<void> refreshAll() async {
    final app = ref.read(appProvider.notifier);
    await app.runOperation('Loading permissions', () async {
      final results = await Future.wait([api.listUsers(), api.listTeams(), api.listApiKeys()]);
      state = state.copyWith(
        users: results[0] as List<User>,
        teams: results[1] as List<Team>,
        apiKeys: results[2] as List<ApiKey>,
      );
    });

    if (state.selectedUserId != null && selectedUser() == null) {
      state = state.copyWith(selectedUserId: null);
    }

    if (state.selectedTeamId != null && selectedTeam() == null) {
      state = state.copyWith(selectedTeamId: null);
    }

    if (state.selectedApiKeyId != null && selectedApiKey() == null) {
      state = state.copyWith(selectedApiKeyId: null);
    }

    if (selectedUser() != null) {
      await refreshSelectedUserTeams();
    }

    if (selectedTeam() != null) {
      await refreshSelectedTeamMembers();
    }
  }

  Future<void> refreshApiKeys() async {
    final app = ref.read(appProvider.notifier);
    final apiKeys = await app.runOperation('Loading API keys', api.listApiKeys);
    state = state.copyWith(apiKeys: apiKeys);

    if (state.selectedApiKeyId != null && selectedApiKey() == null) {
      state = state.copyWith(selectedApiKeyId: null, revealedApiKey: null);
    }
  }

  void clearPermissions() {
    state = _initialPermissionsState();
  }

  UserDraft selectUser(User? user) {
    state = state.copyWith(selectedUserId: user?.id, userTeams: user?.id != null ? state.userTeams : const []);

    if (user?.id != null) {
      unawaited(refreshSelectedUserTeams());
    }

    return user != null ? userDraftFrom(user) : blankUserDraft();
  }

  UserDraft clearUserSelection() {
    state = state.copyWith(selectedUserId: null, userTeams: const []);
    return blankUserDraft();
  }

  Future<void> saveUserDraft(UserDraft userDraft) async {
    final app = ref.read(appProvider.notifier);
    final username = userDraft.username.trim();
    final email = userDraft.email.trim();
    final currentUser = selectedUser();

    if (currentUser == null && username.isEmpty) {
      app.setError('Username is required.');
      return;
    }

    if (currentUser == null && userDraft.password.isEmpty) {
      app.setError('Password is required for new users.');
      return;
    }

    final editing = currentUser != null;

    final saved = await app.runOperation(editing ? 'Updating user' : 'Creating user', () async {
      if (currentUser?.id != null) {
        final request = api.UpdateUserInput(
          email: email.isNotEmpty ? email : null,
          isAdmin: userDraft.isAdmin,
          disabled: userDraft.disabled,
          password: userDraft.password.isNotEmpty ? userDraft.password : null,
        );
        return api.updateUser(currentUser!.id!, request);
      }

      final request = api.CreateUserInput(
        username: username,
        password: userDraft.password,
        email: email.isNotEmpty ? email : null,
        isAdmin: userDraft.isAdmin,
      );
      return api.createUser(request);
    });

    await refreshAll();
    final match = state.users.where((u) => u.id == saved.id);
    selectUser(match.isNotEmpty ? match.first : saved);
    app.setStatus(editing ? 'User saved.' : 'User created.');
  }

  Future<void> deleteSelectedUser() async {
    final app = ref.read(appProvider.notifier);
    final user = selectedUser();

    if (user?.id == null) {
      return;
    }

    await app.runOperation('Deleting user', () => api.deleteUser(user!.id!));
    clearUserSelection();
    await refreshAll();
    app.setStatus('User deleted.');
  }

  ApiKeyDraft selectApiKey(ApiKey? apiKey, String? selectedUserId) {
    state = state.copyWith(selectedApiKeyId: apiKey?.id, revealedApiKey: null);
    return apiKey != null ? apiKeyDraftFrom(apiKey) : blankApiKeyDraft(selectedUserId);
  }

  ApiKeyDraft clearApiKeySelection(String? selectedUserId) {
    state = state.copyWith(selectedApiKeyId: null, revealedApiKey: null);
    return blankApiKeyDraft(selectedUserId);
  }

  Future<void> saveApiKeyDraft(ApiKeyDraft apiKeyDraft) async {
    final app = ref.read(appProvider.notifier);
    final name = apiKeyDraft.name.trim();

    if (name.isEmpty) {
      app.setError('API key name is required.');
      return;
    }

    final currentApiKey = selectedApiKey();
    final editing = currentApiKey?.id != null;
    final selectedUserId = state.selectedUserId;

    final saved = await app.runOperation(editing ? 'Updating API key' : 'Creating API key', () async {
      if (currentApiKey?.id != null) {
        final request = api.UpdateApiKeyInput(
          name: name,
          expiresAt: apiKeyDraft.expiresAt.isNotEmpty ? DateTime.parse(apiKeyDraft.expiresAt).toUtc().toIso8601String() : null,
          disabled: apiKeyDraft.disabled,
        );
        return api.updateApiKey(currentApiKey!.id!, request);
      }

      final request = api.CreateApiKeyInput(
        name: name,
        isService: apiKeyDraft.isService,
        userId: apiKeyDraft.isService ? null : (apiKeyDraft.userId.isNotEmpty ? apiKeyDraft.userId : selectedUserId),
        expiresAt: apiKeyDraft.expiresAt.isNotEmpty ? DateTime.parse(apiKeyDraft.expiresAt).toUtc().toIso8601String() : null,
      );
      final created = await api.createApiKey(request);
      state = state.copyWith(revealedApiKey: created);
      return created.apiKey;
    });

    await refreshApiKeys();
    final reveal = state.revealedApiKey;
    final match = state.apiKeys.where((k) => k.id == saved.id);
    selectApiKey(match.isNotEmpty ? match.first : saved, selectedUserId);

    if (reveal != null) {
      state = state.copyWith(revealedApiKey: reveal);
    }

    if (!editing && reveal != null) {
      state = state.copyWith(selectedApiKeyId: reveal.apiKey.id);
    }

    app.setStatus(editing ? 'API key saved.' : 'API key created.');
  }

  Future<void> revokeSelectedApiKey() async {
    final app = ref.read(appProvider.notifier);
    final apiKeyId = selectedApiKey()?.id;

    if (apiKeyId == null) {
      return;
    }

    await app.runOperation('Revoking API key', () => api.revokeApiKey(apiKeyId));
    await refreshApiKeys();

    final refreshedList = state.apiKeys.where((k) => k.id == apiKeyId);

    if (refreshedList.isNotEmpty) {
      selectApiKey(refreshedList.first, state.selectedUserId);
    } else {
      clearApiKeySelection(state.selectedUserId);
    }

    app.setStatus('API key revoked.');
  }

  Future<void> rotateSelectedApiKey() async {
    final app = ref.read(appProvider.notifier);
    final apiKeyId = selectedApiKey()?.id;

    if (apiKeyId == null) {
      return;
    }

    final rotated = await app.runOperation('Rotating API key', () => api.rotateApiKey(apiKeyId));
    state = state.copyWith(revealedApiKey: rotated);
    await refreshApiKeys();
    final match = state.apiKeys.where((k) => k.id == rotated.apiKey.id);
    selectApiKey(match.isNotEmpty ? match.first : rotated.apiKey, state.selectedUserId);
    state = state.copyWith(revealedApiKey: rotated);
    app.setStatus('API key rotated.');
  }

  Future<void> refreshSelectedUserTeams() async {
    final app = ref.read(appProvider.notifier);
    final userId = selectedUser()?.id;

    if (userId == null) {
      state = state.copyWith(userTeams: const []);
      return;
    }

    final userTeams = await app.runOperation('Loading user teams', () => api.listUserTeams(userId));
    state = state.copyWith(userTeams: userTeams);
  }

  Future<void> assignSelectedUserToTeam(String teamId) async {
    final app = ref.read(appProvider.notifier);
    final userId = selectedUser()?.id;

    if (userId == null || teamId.isEmpty) {
      return;
    }

    await app.runOperation('Assigning team', () => api.addTeamMember(teamId, userId));
    await Future.wait([
      refreshSelectedUserTeams(),
      if (selectedTeam() != null) refreshSelectedTeamMembers() else Future.value(),
    ]);
    app.setStatus('Team assigned.');
  }

  Future<void> removeSelectedUserFromTeam(String teamId) async {
    final app = ref.read(appProvider.notifier);
    final userId = selectedUser()?.id;

    if (userId == null || teamId.isEmpty) {
      return;
    }

    await app.runOperation('Removing team', () => api.removeTeamMember(teamId, userId));
    await Future.wait([
      refreshSelectedUserTeams(),
      if (selectedTeam() != null) refreshSelectedTeamMembers() else Future.value(),
    ]);
    app.setStatus('Team removed.');
  }

  String selectTeam(Team? team) {
    state = state.copyWith(selectedTeamId: team?.id, teamMembers: team?.id != null ? state.teamMembers : const []);

    if (team?.id != null) {
      unawaited(refreshSelectedTeamMembers());
    }

    return team?.name ?? '';
  }

  String clearTeamSelection() {
    state = state.copyWith(selectedTeamId: null, teamMembers: const []);
    return '';
  }

  Future<void> saveTeamDraft(String teamDraftName) async {
    final app = ref.read(appProvider.notifier);
    final name = teamDraftName.trim();

    if (name.isEmpty) {
      app.setError('Team name is required.');
      return;
    }

    final currentTeam = selectedTeam();
    final editing = currentTeam != null;
    final saved = await app.runOperation(
      editing ? 'Updating team' : 'Creating team',
      () => currentTeam?.id != null ? api.updateTeam(currentTeam!.id!, name) : api.createTeam(name),
    );
    await refreshAll();
    final match = state.teams.where((t) => t.id == saved.id);
    selectTeam(match.isNotEmpty ? match.first : saved);
    app.setStatus(editing ? 'Team saved.' : 'Team created.');
  }

  Future<void> deleteSelectedTeam() async {
    final app = ref.read(appProvider.notifier);
    final team = selectedTeam();

    if (team?.id == null) {
      return;
    }

    await app.runOperation('Deleting team', () => api.deleteTeam(team!.id!));
    clearTeamSelection();
    await refreshAll();
    app.setStatus('Team deleted.');
  }

  Future<void> refreshSelectedTeamMembers() async {
    final app = ref.read(appProvider.notifier);
    final teamId = selectedTeam()?.id;

    if (teamId == null) {
      state = state.copyWith(teamMembers: const []);
      return;
    }

    final teamMembers = await app.runOperation('Loading team members', () => api.listTeamMembers(teamId));
    state = state.copyWith(teamMembers: teamMembers);
  }

  Future<void> addSelectedTeamMember(String userId) async {
    final app = ref.read(appProvider.notifier);
    final teamId = selectedTeam()?.id;

    if (teamId == null || userId.isEmpty) {
      return;
    }

    await app.runOperation('Adding member', () => api.addTeamMember(teamId, userId));
    await Future.wait([
      refreshSelectedTeamMembers(),
      if (selectedUser() != null) refreshSelectedUserTeams() else Future.value(),
    ]);
    app.setStatus('Member added.');
  }

  Future<void> removeSelectedTeamMember(String userId) async {
    final app = ref.read(appProvider.notifier);
    final teamId = selectedTeam()?.id;

    if (teamId == null || userId.isEmpty) {
      return;
    }

    await app.runOperation('Removing member', () => api.removeTeamMember(teamId, userId));
    await Future.wait([
      refreshSelectedTeamMembers(),
      if (selectedUser() != null) refreshSelectedUserTeams() else Future.value(),
    ]);
    app.setStatus('Member removed.');
  }

  Future<GrantDraft> selectWorkflow(String? workflowId) async {
    state = state.copyWith(selectedWorkflowId: workflowId, workflowGrants: workflowId != null ? state.workflowGrants : const []);

    if (workflowId != null) {
      await refreshWorkflowGrants();
    }

    return blankGrantDraft();
  }

  Future<void> refreshWorkflowGrants() async {
    final app = ref.read(appProvider.notifier);
    final workflowId = state.selectedWorkflowId;

    if (workflowId == null) {
      state = state.copyWith(workflowGrants: const []);
      return;
    }

    final grantsJson = await app.runOperation('Loading workflow access', () => api.listWorkflowGrants(workflowId));
    state = state.copyWith(workflowGrants: grantsJson.map((json) => Grant.fromJson(json)).toList());
  }

  Future<GrantDraft> saveGrantDraft(GrantDraft grantDraft) async {
    final app = ref.read(appProvider.notifier);
    final workflowId = state.selectedWorkflowId;

    if (workflowId == null) {
      app.setError('Select a workflow first.');
      return blankGrantDraft();
    }

    if (grantDraft.principalId.isEmpty) {
      app.setError('Select a user or team first.');
      return grantDraft;
    }

    await app.runOperation(
      'Saving access',
      () => api.grantWorkflowAccess(workflowId, grantDraft.principalType, grantDraft.principalId, grantDraft.permission),
    );
    await refreshWorkflowGrants();
    app.setStatus('Access saved.');
    return blankGrantDraft();
  }

  Future<void> revokeGrant(String? grantId) async {
    final app = ref.read(appProvider.notifier);
    final workflowId = state.selectedWorkflowId;

    if (workflowId == null || grantId == null) {
      return;
    }

    await app.runOperation('Revoking access', () => api.revokeWorkflowGrant(workflowId, grantId));
    await refreshWorkflowGrants();
    app.setStatus('Access revoked.');
  }
}

String _userSearchText(User user) => [
      user.id,
      user.username,
      user.email,
      user.isAdmin ? 'admin' : 'user',
      user.disabled ? 'disabled' : 'enabled',
    ].where((v) => v != null && v.isNotEmpty).join(' ').toLowerCase();

String _teamSearchText(Team team) => [team.id, team.name].where((v) => v != null && v.isNotEmpty).join(' ').toLowerCase();

String _apiKeySearchText(ApiKey apiKey, List<User> users) {
  String? owner;
  if (apiKey.userId != null) {
    final match = users.where((u) => u.id == apiKey.userId);
    owner = match.isNotEmpty ? match.first.username : null;
  } else {
    owner = 'service';
  }

  return [
    apiKey.id,
    apiKey.name,
    apiKey.keyPrefix,
    owner,
    apiKey.isService ? 'service' : 'user',
    apiKey.disabled ? 'disabled' : 'active',
  ].where((v) => v != null && v.isNotEmpty).join(' ').toLowerCase();
}

String _toDateTimeInput(String value) {
  final date = DateTime.tryParse(value);

  if (date == null) {
    return '';
  }

  return date.toIso8601String().substring(0, 16);
}
