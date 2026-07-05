import 'package:flutter/material.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/utils/secrets.dart';
import '../../core/utils/settings_tree.dart';
import '../theme/app_theme.dart';
import 'cc_widgets.dart';

class SettingsTreeWidget extends StatelessWidget {
  const SettingsTreeWidget({
    super.key,
    required this.nodes,
    required this.selectedKey,
    required this.onSelect,
    this.configValues = const {},
    this.isConfig = false,
    this.depth = 0,
  });

  final List<SettingsTreeNode> nodes;
  final String selectedKey;
  final ValueChanged<CredentialSummary> onSelect;
  final Map<String, Object?> configValues;
  final bool isConfig;
  final int depth;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final node in nodes)
          switch (node) {
            SettingsTreeFolder folder => _FolderTile(
                folder: folder,
                selectedKey: selectedKey,
                onSelect: onSelect,
                configValues: configValues,
                isConfig: isConfig,
                depth: depth,
              ),
            SettingsTreeLeaf leaf => _LeafTile(
                leaf: leaf,
                selected: selectedKey == secretKey(leaf.setting),
                onSelect: () => onSelect(leaf.setting),
                configValues: configValues,
                isConfig: isConfig,
                depth: depth,
              ),
          },
      ],
    );
  }
}

class _FolderTile extends StatefulWidget {
  const _FolderTile({
    required this.folder,
    required this.selectedKey,
    required this.onSelect,
    required this.configValues,
    required this.isConfig,
    required this.depth,
  });

  final SettingsTreeFolder folder;
  final String selectedKey;
  final ValueChanged<CredentialSummary> onSelect;
  final Map<String, Object?> configValues;
  final bool isConfig;
  final int depth;

  @override
  State<_FolderTile> createState() => _FolderTileState();
}

class _FolderTileState extends State<_FolderTile> {
  var _expanded = true;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InkWell(
          onTap: () => setState(() => _expanded = !_expanded),
          child: Padding(
            padding: EdgeInsets.fromLTRB(8.0 + widget.depth * 12, 6, 8, 6),
            child: Row(
              children: [
                Icon(_expanded ? Icons.expand_more : Icons.chevron_right, size: 16, color: AppColors.textMuted),
                CcIcon(IconName.folder, size: 14),
                const SizedBox(width: 6),
                Expanded(child: Text(widget.folder.label, style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 12))),
                Text('${widget.folder.children.length}', style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
              ],
            ),
          ),
        ),
        if (_expanded)
          SettingsTreeWidget(
            nodes: widget.folder.children,
            selectedKey: widget.selectedKey,
            onSelect: widget.onSelect,
            configValues: widget.configValues,
            isConfig: widget.isConfig,
            depth: widget.depth + 1,
          ),
      ],
    );
  }
}

class _LeafTile extends StatelessWidget {
  const _LeafTile({
    required this.leaf,
    required this.selected,
    required this.onSelect,
    required this.configValues,
    required this.isConfig,
    required this.depth,
  });

  final SettingsTreeLeaf leaf;
  final bool selected;
  final VoidCallback onSelect;
  final Map<String, Object?> configValues;
  final bool isConfig;
  final int depth;

  @override
  Widget build(BuildContext context) {
    final key = secretKey(leaf.setting);
    final preview = isConfig ? configValues[key]?.toString() : null;

    return Material(
      color: selected ? AppColors.accentSoft : Colors.transparent,
      child: InkWell(
        onTap: onSelect,
        child: Padding(
          padding: EdgeInsets.fromLTRB(8.0 + depth * 12 + 16, 6, 8, 6),
          child: Row(
            children: [
              CcIcon(isConfig ? IconName.settings : IconName.key, size: 14),
              const SizedBox(width: 6),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(leaf.label, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
                    Text(key, style: TextStyle(fontSize: 10, color: AppColors.textMuted)),
                    if (preview != null)
                      Text(preview, style: TextStyle(fontSize: 11, color: AppColors.textSubtle), maxLines: 1, overflow: TextOverflow.ellipsis),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
