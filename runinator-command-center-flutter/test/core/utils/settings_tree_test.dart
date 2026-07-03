import 'package:runinator_command_center_flutter/core/domain/models/credential.dart';
import 'package:runinator_command_center_flutter/core/domain/models/setting.dart';
import 'package:runinator_command_center_flutter/core/utils/settings_tree.dart';
import 'package:test/test.dart';

CredentialSummary setting(String scope, String name, [SettingKind kind = SettingKind.config]) =>
    CredentialSummary(scope: scope, name: name, kind: kind);

void main() {
  group('buildSettingsTree', () {
    test('groups settings by scope into folders with leaves', () {
      final tree = buildSettingsTree([
        setting('github', 'token'),
        setting('github', 'webhook_secret'),
        setting('foreign_languages', 'python'),
      ]);

      expect(tree.map((node) => node.path), ['foreign_languages', 'github']);
      final github = tree.firstWhere((node) => node.path == 'github') as SettingsTreeFolder;
      expect(github.children.map((child) => child.label), ['token', 'webhook_secret']);
      expect(github.children.every((child) => child is SettingsTreeLeaf), isTrue);
    });

    test('splits dotted names into nested folders', () {
      final tree = buildSettingsTree([setting('database', 'primary.host')]);
      final database = tree[0] as SettingsTreeFolder;
      expect(database.path, 'database');
      final primary = database.children[0] as SettingsTreeFolder;
      expect(primary.path, 'database.primary');
      expect(primary.children[0], isA<SettingsTreeLeaf>());
      final host = primary.children[0] as SettingsTreeLeaf;
      expect(host.label, 'host');
      expect(host.path, 'database.primary.host');
    });

    test('orders folders before leaves at the same level', () {
      final tree = buildSettingsTree([setting('api', 'url'), setting('api', 'nested.value')]);
      final api = tree[0] as SettingsTreeFolder;
      expect(api.children.map((child) => child is SettingsTreeFolder ? 'folder' : 'leaf'), ['folder', 'leaf']);
      expect(api.children.map((child) => child.label), ['nested', 'url']);
    });

    test('ignores entries with empty paths', () {
      expect(buildSettingsTree([setting('', '')]), isEmpty);
    });
  });
}
