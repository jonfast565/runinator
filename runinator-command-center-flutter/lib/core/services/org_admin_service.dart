// port of core/services/org-admin.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import '../domain/json.dart';
import '../domain/models/index.dart';
import 'app_service.dart';

part 'org_admin_service.g.dart';

class OrgAdminService {
  const OrgAdminService(this._app);

  final AppNotifier _app;

  Future<List<api.OrgMembership>> listMembers(String orgId) =>
      _app.runOperation('Loading org members', () => api.listOrgMembers(orgId));

  Future<JsonRecord> addMember(String orgId, String userId, api.OrgRole role) =>
      _app.runOperation('Adding org member', () => api.addOrgMember(orgId, userId, role));

  Future<JsonRecord> updateMember(String orgId, String userId, api.OrgRole role) =>
      _app.runOperation('Updating org member', () => api.updateOrgMember(orgId, userId, role));

  Future<JsonRecord> removeMember(String orgId, String userId) =>
      _app.runOperation('Removing org member', () => api.removeOrgMember(orgId, userId));

  Future<List<User>> listUsers() => _app.runOperation('Loading users', api.listUsers);

  Future<List<Team>> listTeams() => _app.runOperation('Loading teams', api.listTeams);

  Future<Team> createTeam(String name) => _app.runOperation('Creating team', () => api.createTeam(name));

  Future<TaskResponse> deleteTeam(String teamId) => _app.runOperation('Deleting team', () => api.deleteTeam(teamId));

  Future<List<User>> listTeamMembers(String teamId) =>
      _app.runOperation('Loading team members', () => api.listTeamMembers(teamId));

  Future<TaskResponse> addTeamMember(String teamId, String userId) =>
      _app.runOperation('Adding team member', () => api.addTeamMember(teamId, userId));

  Future<TaskResponse> removeTeamMember(String teamId, String userId) =>
      _app.runOperation('Removing team member', () => api.removeTeamMember(teamId, userId));
}

@riverpod
OrgAdminService orgAdminService(Ref ref) => OrgAdminService(ref.watch(appProvider.notifier));
