// port of core/services/orgs.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart' as api;
import 'app_service.dart';
import 'auth_service.dart';

part 'orgs_service.g.dart';

const String _activeOrgKey = 'runinator.org.active';

const Map<api.OrgRole, int> orgRoleRank = {api.OrgRole.member: 0, api.OrgRole.admin: 1, api.OrgRole.owner: 2};

/// core/ has no browser localStorage dependency; a concrete web platform adapter
/// (future UI pass) supplies persistence via [setOrgsStorage].
String? Function(String key)? _storageReader;
void Function(String key, String? value)? _storageWriter;

void setOrgsStorage({String? Function(String key)? reader, void Function(String key, String? value)? writer}) {
  _storageReader = reader;
  _storageWriter = writer;
}

String? _safeGet(String key) => _storageReader?.call(key);

void _safeSet(String key, String? value) => _storageWriter?.call(key, value);

class OrgsState {
  const OrgsState({required this.memberships, this.activeOrgId});

  final List<api.OrgMembershipView> memberships;
  final String? activeOrgId;

  OrgsState copyWith({List<api.OrgMembershipView>? memberships, Object? activeOrgId = _unset}) => OrgsState(
        memberships: memberships ?? this.memberships,
        activeOrgId: identical(activeOrgId, _unset) ? this.activeOrgId : activeOrgId as String?,
      );
}

const Object _unset = Object();

@riverpod
class OrgsNotifier extends _$OrgsNotifier {
  @override
  OrgsState build() => OrgsState(memberships: const [], activeOrgId: _safeGet(_activeOrgKey));

  api.OrgMembershipView? activeMembership() {
    for (final membership in state.memberships) {
      if (membership.org.id == state.activeOrgId) {
        return membership;
      }
    }
    return null;
  }

  api.Organization? activeOrg() => activeMembership()?.org;

  api.OrgRole? activeRole() => activeMembership()?.role;

  bool isActiveOrgAdmin() {
    final role = activeRole();
    return role != null && orgRoleRank[role]! >= orgRoleRank[api.OrgRole.admin]!;
  }

  bool hasOrgs() => state.memberships.isNotEmpty;

  void setActiveLocal(String? orgId) {
    state = state.copyWith(activeOrgId: orgId);
    _safeSet(_activeOrgKey, orgId);
  }

  Future<void> refresh() async {
    final app = ref.read(appProvider.notifier);
    List<api.OrgMembershipView> memberships;
    try {
      memberships = await app.runOperation('Loading organizations', api.listMyOrgs);
    } catch (_) {
      memberships = [];
    }

    var activeOrgId = state.activeOrgId;

    if (activeOrgId != null && !memberships.any((m) => m.org.id == activeOrgId)) {
      setActiveLocal(null);
      activeOrgId = null;
    }

    state = state.copyWith(memberships: memberships);

    if (activeOrgId == null && memberships.isNotEmpty) {
      await setActive(memberships.first.org.id);
    }
  }

  Future<bool> setActive(String orgId) async {
    final app = ref.read(appProvider.notifier);
    final auth = ref.read(authProvider.notifier);

    try {
      final context = await api.switchOrg(orgId);
      await auth.applyAccessToken(context.accessToken);
      setActiveLocal(orgId);
      app.setStatus('Active organization: ${context.org.name}');
      return true;
    } catch (err) {
      app.setError(err.toString());
      return false;
    }
  }

  Future<bool> create(String name) async {
    final app = ref.read(appProvider.notifier);
    api.Organization? org;
    try {
      org = await app.runOperation('Creating organization', () => api.createOrg(name));
    } catch (_) {
      org = null;
    }

    if (org == null) {
      return false;
    }

    await refresh();
    await setActive(org.id);
    return true;
  }

  void clear() {
    state = state.copyWith(memberships: const []);
    setActiveLocal(null);
  }
}
