import 'package:runinator_command_center_flutter/core/api/command_center_api.dart';
import 'package:runinator_command_center_flutter/core/api/command_runtime.dart';
import 'package:runinator_command_center_flutter/core/api/http_runtime.dart';
import 'package:test/test.dart';

class _RecordingRuntime implements CommandRuntime {
  final calls = <({String name, Map<String, Object?>? args})>[];

  @override
  bool isTauri() => false;

  @override
  Future<Object?> invoke(String name, [Map<String, Object?>? args]) async {
    calls.add((name: name, args: args));
    if (name.startsWith('fetch_') || name.startsWith('list_')) {
      return <Object?>[];
    }
    return <String, Object?>{};
  }

  @override
  String wsBaseUrl() => 'http://127.0.0.1:8080';

  @override
  String apiBaseUrl() => '/api';
}

void main() {
  group('command center workflow node run API', () {
    late _RecordingRuntime runtime;

    setUp(() {
      runtime = _RecordingRuntime();
      setCommandRuntime(runtime);
    });

    test('requests workflow node run chunks by node run id', () async {
      await fetchWorkflowNodeRunChunks('00000000-0000-0000-0000-000000000042');

      expect(runtime.calls.single.name, 'fetch_workflow_node_run_chunks');
      expect(runtime.calls.single.args, {
        'nodeRunId': '00000000-0000-0000-0000-000000000042',
      });
    });

    test('requests workflow node run artifacts by node run id', () async {
      await fetchWorkflowNodeRunArtifacts('00000000-0000-0000-0000-000000000042');

      expect(runtime.calls.single.name, 'fetch_workflow_node_run_artifacts');
      expect(runtime.calls.single.args, {
        'nodeRunId': '00000000-0000-0000-0000-000000000042',
      });
    });
  });

  group('command center permissions API in web mode', () {
    test('maps user creation to the users endpoint', () {
      final descriptor = httpRegistry['create_user']!;
      final body = descriptor.body!({
        'request': {
          'username': 'ada',
          'password': 'secret',
          'email': 'ada@example.com',
          'is_admin': true,
        },
      });

      expect(descriptor.method(null), HttpMethod.post);
      expect(descriptor.path(null), 'users');
      expect(body, {
        'username': 'ada',
        'password': 'secret',
        'email': 'ada@example.com',
        'is_admin': true,
      });
    });

    test('maps team rename and membership endpoints', () {
      final updateTeam = httpRegistry['update_team']!;
      expect(updateTeam.method({'teamId': 't1', 'name': 'platform'}), HttpMethod.patch);
      expect(updateTeam.path({'teamId': '00000000-0000-0000-0000-000000000001', 'name': 'platform'}),
          'teams/00000000-0000-0000-0000-000000000001');
      expect(
        updateTeam.body!({'teamId': '00000000-0000-0000-0000-000000000001', 'name': 'platform'}),
        {'name': 'platform'},
      );

      final addMember = httpRegistry['add_team_member']!;
      expect(addMember.method(null), HttpMethod.post);
      expect(
        addMember.path({'teamId': '00000000-0000-0000-0000-000000000001', 'userId': 'u2'}),
        'teams/00000000-0000-0000-0000-000000000001/members',
      );
      expect(
        addMember.body!({
          'teamId': '00000000-0000-0000-0000-000000000001',
          'userId': '00000000-0000-0000-0000-000000000002',
        }),
        {'user_id': '00000000-0000-0000-0000-000000000002'},
      );

      final listMembers = httpRegistry['list_team_members']!;
      expect(listMembers.method(null), HttpMethod.get);
      expect(
        listMembers.path({'teamId': '00000000-0000-0000-0000-000000000001'}),
        'teams/00000000-0000-0000-0000-000000000001/members',
      );
    });

    test('maps api key lifecycle endpoints', () {
      final createKey = httpRegistry['create_api_key']!;
      expect(createKey.method(null), HttpMethod.post);
      expect(createKey.path(null), 'api_keys');
      expect(
        createKey.body!({
          'request': {
            'name': 'deploy',
            'user_id': '00000000-0000-0000-0000-000000000002',
            'is_service': false,
            'expires_at': null,
          },
        }),
        {
          'name': 'deploy',
          'user_id': '00000000-0000-0000-0000-000000000002',
          'is_service': false,
          'expires_at': null,
        },
      );

      final updateKey = httpRegistry['update_api_key']!;
      expect(updateKey.method(null), HttpMethod.patch);
      expect(
        updateKey.path({'keyId': '00000000-0000-0000-0000-000000000003'}),
        'api_keys/00000000-0000-0000-0000-000000000003',
      );
      expect(
        updateKey.body!({
          'keyId': '00000000-0000-0000-0000-000000000003',
          'request': {
            'name': 'deploy renamed',
            'expires_at': null,
            'disabled': false,
          },
        }),
        {
          'name': 'deploy renamed',
          'expires_at': null,
          'disabled': false,
        },
      );

      final rotateKey = httpRegistry['rotate_api_key']!;
      expect(rotateKey.method({'keyId': '00000000-0000-0000-0000-000000000003'}), HttpMethod.post);
      expect(
        rotateKey.path({'keyId': '00000000-0000-0000-0000-000000000003'}),
        'api_keys/00000000-0000-0000-0000-000000000003/rotate',
      );
    });
  });
}
