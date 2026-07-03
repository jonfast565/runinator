// port of core/utils/settings-tree.ts.

import '../domain/models/index.dart';

/// a leaf is a concrete setting; a folder groups settings that share a dotted-path prefix.
sealed class SettingsTreeNode {
  const SettingsTreeNode({required this.label, required this.path});

  final String label;
  final String path;
}

class SettingsTreeLeaf extends SettingsTreeNode {
  const SettingsTreeLeaf({required super.label, required super.path, required this.setting});

  final CredentialSummary setting;
}

class SettingsTreeFolder extends SettingsTreeNode {
  const SettingsTreeFolder({required super.label, required super.path, required this.children});

  final List<SettingsTreeNode> children;
}

class _FolderBuilder {
  _FolderBuilder(this.path);

  final String path;
  final Map<String, _FolderBuilder> folders = {};
  final List<SettingsTreeLeaf> leaves = [];
}

// the dotted path of a setting is its scope segments followed by its name segments.
List<String> _settingSegments(CredentialSummary setting) => '${setting.scope}.${setting.name}'
    .split('.')
    .map((segment) => segment.trim())
    .where((segment) => segment.isNotEmpty)
    .toList();

String _joinPath(String prefix, String segment) => prefix.isNotEmpty ? '$prefix.$segment' : segment;

List<SettingsTreeNode> _finalizeFolder(_FolderBuilder builder) {
  final folders = builder.folders.values
      .map((child) => SettingsTreeFolder(
            label: child.path.substring(builder.path.isNotEmpty ? builder.path.length + 1 : 0),
            path: child.path,
            children: _finalizeFolder(child),
          ))
      .toList()
    ..sort((a, b) => a.label.compareTo(b.label));
  final leaves = [...builder.leaves]..sort((a, b) => a.label.compareTo(b.label));
  // folders first, then leaves, each alphabetical.
  return [...folders, ...leaves];
}

// group flat settings into a collapsible dotted-path tree (config.<scope>.<name> shape).
List<SettingsTreeNode> buildSettingsTree(List<CredentialSummary> entries) {
  final root = _FolderBuilder('');

  for (final setting in entries) {
    final segments = _settingSegments(setting);

    if (segments.isEmpty) {
      continue;
    }

    var cursor = root;

    for (var index = 0; index < segments.length - 1; index++) {
      final segment = segments[index];
      final path = _joinPath(cursor.path, segment);
      var next = cursor.folders[segment];

      if (next == null) {
        next = _FolderBuilder(path);
        cursor.folders[segment] = next;
      }

      cursor = next;
    }

    final label = segments.last;
    cursor.leaves.add(SettingsTreeLeaf(label: label, path: _joinPath(cursor.path, label), setting: setting));
  }

  return _finalizeFolder(root);
}
