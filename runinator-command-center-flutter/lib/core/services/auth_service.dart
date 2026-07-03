// port of core/services/auth.ts.

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../api/command_center_api.dart';
import '../domain/json.dart';
import '../platform/index.dart' show getPlatformAdapterOptional;
import '../platform/types.dart' show AuthStorage;

part 'auth_service.g.dart';

const String _accessKey = 'runinator.auth.access';
const String _refreshKey = 'runinator.auth.refresh';

class AuthState {
  const AuthState({
    required this.required,
    required this.authenticated,
    required this.ready,
    this.user,
    required this.error,
    required this.accessTokenRevision,
  });

  final bool required;
  final bool authenticated;
  final bool ready;
  final JsonRecord? user;
  final String error;
  final int accessTokenRevision;

  AuthState copyWith({
    bool? required,
    bool? authenticated,
    bool? ready,
    Object? user = _unset,
    String? error,
    int? accessTokenRevision,
  }) =>
      AuthState(
        required: required ?? this.required,
        authenticated: authenticated ?? this.authenticated,
        ready: ready ?? this.ready,
        user: identical(user, _unset) ? this.user : user as JsonRecord?,
        error: error ?? this.error,
        accessTokenRevision: accessTokenRevision ?? this.accessTokenRevision,
      );
}

const Object _unset = Object();

AuthState _initialAuthState() => const AuthState(
      required: false,
      authenticated: false,
      ready: false,
      user: null,
      error: '',
      accessTokenRevision: 0,
    );

class _FallbackAuthStorage implements AuthStorage {
  final Map<String, String> _memory = {};

  @override
  String? get(String key) => _memory[key];

  @override
  void set(String key, String value) => _memory[key] = value;

  @override
  void remove(String key) => _memory.remove(key);
}

/// mirrors the ts source's localStorage-backed fallbackAuthStorage; since core/
/// has no browser dependency this pass, the fallback is in-memory. the future web
/// platform adapter supplies a real persistent AuthStorage via setPlatformAdapter.
final AuthStorage _fallbackAuthStorage = _FallbackAuthStorage();

AuthStorage _authStorage() => getPlatformAdapterOptional()?.authStorage ?? _fallbackAuthStorage;

String? _safeGet(String key) => _authStorage().get(key);

@riverpod
class AuthNotifier extends _$AuthNotifier {
  String? _refreshToken;

  @override
  AuthState build() => _initialAuthState();

  void resetForTests() {
    _refreshToken = null;
    state = _initialAuthState();
  }

  void _persist(String? access, String? refresh) {
    _refreshToken = refresh;
    final storage = _authStorage();

    if (access != null) {
      storage.set(_accessKey, access);
    } else {
      storage.remove(_accessKey);
    }

    if (refresh != null) {
      storage.set(_refreshKey, refresh);
    } else {
      storage.remove(_refreshKey);
    }
  }

  Future<void> _publishAccessToken(String? access) async {
    await setAccessToken(access);
    state = state.copyWith(accessTokenRevision: state.accessTokenRevision + 1);
  }

  Future<void> _apply(LoginResult result) async {
    _persist(result.accessToken, result.refreshToken);
    await _publishAccessToken(result.accessToken);
    state = state.copyWith(user: result.user, authenticated: true);
  }

  Future<void> _clear() async {
    _persist(null, null);
    await _publishAccessToken(null);
    state = state.copyWith(authenticated: false, user: null);
  }

  Future<bool> _tryRefresh(String token) async {
    try {
      await _apply(await refreshSession(token));
      return true;
    } catch (_) {
      await _clear();
      return false;
    }
  }

  Future<void> init() async {
    try {
      final config = await fetchAuthConfig();
      state = state.copyWith(required: config.enabled);
    } catch (_) {
      state = state.copyWith(required: false);
    }

    final required = state.required;

    if (!required) {
      state = state.copyWith(authenticated: true, ready: true);
      return;
    }

    final access = _safeGet(_accessKey);
    final refresh = _safeGet(_refreshKey);

    if (access != null) {
      _refreshToken = refresh;
      await _publishAccessToken(access);

      try {
        final user = await fetchAuthMe();
        state = state.copyWith(user: user, authenticated: true);
      } catch (_) {
        final authenticated = refresh != null ? await _tryRefresh(refresh) : false;
        state = state.copyWith(authenticated: authenticated);
      }
    }

    state = state.copyWith(ready: true);
  }

  Future<bool> signIn(String username, String password) async {
    state = state.copyWith(error: '');

    try {
      await _apply(await login(username, password));
      return true;
    } catch (err) {
      state = state.copyWith(error: err.toString());
      return false;
    }
  }

  Future<void> signOut() async {
    final refreshToken = _refreshToken;

    if (refreshToken != null) {
      try {
        await logout(refreshToken);
      } catch (_) {
        // best effort.
      }
    }

    await _clear();
  }

  Future<void> applyAccessToken(String access) async {
    _persist(access, _refreshToken);
    await _publishAccessToken(access);
  }
}
