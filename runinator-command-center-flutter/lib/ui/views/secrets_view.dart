import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/domain/models/setting.dart';
import '../../core/services/app_service.dart';
import '../../core/services/secrets_service.dart';
import '../../core/utils/secrets.dart';
import '../../core/utils/settings_tree.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../shared/confirm.dart';
import '../shared/settings_tree_node.dart';
import '../shared/split_pane.dart';

enum SettingKindView { config, secret }

class SecretsView extends ConsumerStatefulWidget {
  const SecretsView({super.key, required this.kind});

  final SettingKindView kind;

  @override
  ConsumerState<SecretsView> createState() => _SecretsViewState();
}

class _SecretsViewState extends ConsumerState<SecretsView> {
  var _editorOpen = false;
  var _draft = blankSecretDraft();
  final _scopeController = TextEditingController();
  final _nameController = TextEditingController();
  final _valueController = TextEditingController();

  bool get _isConfig => widget.kind == SettingKindView.config;

  @override
  void dispose() {
    _scopeController.dispose();
    _nameController.dispose();
    _valueController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(secretsProvider);
    final notifier = ref.read(secretsProvider.notifier);
    final query = ref.read(appProvider.notifier).normalizedSearch;
    final title = _isConfig ? 'Configs' : 'Secrets';
    final targetKind = _isConfig ? SettingKind.config : SettingKind.secret;
    final entries = notifier.filteredSecrets(query).where((secret) => secret.kind == targetKind).toList();
    final tree = buildSettingsTree(entries);
    final selected = notifier.selectedSecret();

    return Stack(
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: SplitPane(
            initialFirstFraction: 0.38,
            mobileShowSecond: selected != null,
            mobileBackTitle: title,
            onMobileBack: () => notifier.clearSelection(),
            first: PanelCard(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  PanelToolbar(
                    title: title,
                    actions: [
                      CcButton(icon: IconName.plus, label: 'New', dense: true, onPressed: () => _openNew(notifier)),
                      CcButton(icon: IconName.refresh, label: 'Refresh', dense: true, onPressed: () => _refresh(notifier, entries)),
                    ],
                  ),
                  Expanded(
                    child: entries.isEmpty
                        ? EmptyState(message: 'No $title entries.')
                        : SettingsTreeWidget(
                            nodes: tree,
                            selectedKey: state.selectedSecretKey,
                            configValues: state.configValues.map((k, v) => MapEntry(k, v as Object?)),
                            isConfig: _isConfig,
                            onSelect: (setting) => _selectOverview(notifier, setting),
                          ),
                  ),
                ],
              ),
            ),
            second: selected == null
                ? EmptyState(message: 'Select a $title entry.')
                : PanelCard(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        PanelToolbar(
                          title: selected.name,
                          actions: [
                            CcButton(icon: IconName.edit, label: 'Edit', dense: true, onPressed: () => _openEdit(notifier, selected)),
                          ],
                        ),
                        Text('Scope: ${selected.scope}', style: const TextStyle(fontSize: 12)),
                        Text('Key: ${secretKey(selected)}', style: const TextStyle(fontSize: 11, color: Colors.grey)),
                        const SizedBox(height: 12),
                        if (_isConfig)
                          Expanded(
                            child: JsonEditor(
                              value: state.configValues[secretKey(selected)] ?? '',
                              readOnly: true,
                              onChanged: (_) {},
                            ),
                          )
                        else
                          const Text('Value: ••••••••'),
                      ],
                    ),
                  ),
          ),
        ),
        if (_editorOpen) _SettingEditorModal(
          isConfig: _isConfig,
          scopeController: _scopeController,
          nameController: _nameController,
          valueController: _valueController,
          onClose: () => setState(() => _editorOpen = false),
          onSave: () => _save(notifier),
          onDelete: selected == null ? null : () => _delete(notifier, context),
        ),
      ],
    );
  }

  Future<void> _refresh(SecretsNotifier notifier, List<CredentialSummary> entries) async {
    await notifier.refreshSecrets();
    if (_isConfig) {
      await notifier.loadConfigValues(entries);
    }
  }

  void _openNew(SecretsNotifier notifier) {
    notifier.clearSelection();
    _draft = blankSecretDraft(_isConfig ? SettingKind.config : SettingKind.secret);
    _scopeController.text = '';
    _nameController.text = '';
    _valueController.text = _isConfig ? '{}' : '';
    setState(() => _editorOpen = true);
  }

  Future<void> _selectOverview(SecretsNotifier notifier, CredentialSummary setting) async {
    notifier.selectSecret(setting);
    if (_isConfig) {
      await notifier.loadConfigValue(setting);
    }
  }

  Future<void> _openEdit(SecretsNotifier notifier, CredentialSummary setting) async {
    await _selectOverview(notifier, setting);
    _draft = SecretDraft(
      scope: setting.scope,
      name: setting.name,
      secret: _isConfig ? ref.read(secretsProvider).configValues[secretKey(setting)] ?? '{}' : '',
      kind: setting.kind ?? SettingKind.secret,
    );
    _scopeController.text = _draft.scope;
    _nameController.text = _draft.name;
    _valueController.text = _draft.secret;
    setState(() => _editorOpen = true);
  }

  Future<void> _save(SecretsNotifier notifier) async {
    _draft = SecretDraft(
      scope: _scopeController.text,
      name: _nameController.text,
      secret: _valueController.text,
      kind: _isConfig ? SettingKind.config : SettingKind.secret,
    );
    final ok = await notifier.saveDraft(_draft);
    if (ok && mounted) {
      setState(() => _editorOpen = false);
    }
  }

  Future<void> _delete(SecretsNotifier notifier, BuildContext context) async {
    final confirm = FlutterConfirmContext(context);
    final ok = await confirm.confirmAsync('Delete this setting?');
    if (!ok) return;
    await notifier.deleteSelectedSecret();
    if (mounted) setState(() => _editorOpen = false);
  }
}

class _SettingEditorModal extends StatelessWidget {
  const _SettingEditorModal({
    required this.isConfig,
    required this.scopeController,
    required this.nameController,
    required this.valueController,
    required this.onClose,
    required this.onSave,
    this.onDelete,
  });

  final bool isConfig;
  final TextEditingController scopeController;
  final TextEditingController nameController;
  final TextEditingController valueController;
  final VoidCallback onClose;
  final VoidCallback onSave;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.black54,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 640, maxHeight: 560),
          child: Card(
            margin: const EdgeInsets.all(24),
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Expanded(child: Text(isConfig ? 'Edit Config' : 'Edit Secret', style: const TextStyle(fontWeight: FontWeight.w700))),
                      IconButton(icon: const Icon(Icons.close), onPressed: onClose),
                    ],
                  ),
                  TextField(controller: scopeController, decoration: const InputDecoration(labelText: 'Scope')),
                  const SizedBox(height: 8),
                  TextField(controller: nameController, decoration: const InputDecoration(labelText: 'Name')),
                  const SizedBox(height: 8),
                  Expanded(
                    child: isConfig
                        ? JsonEditor(value: valueController.text, onChanged: (v) => valueController.text = v)
                        : TextField(controller: valueController, maxLines: null, expands: true, decoration: const InputDecoration(labelText: 'Secret value')),
                  ),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    children: [
                      if (onDelete != null)
                        CcButton(icon: IconName.trash, label: 'Delete', variant: CcButtonVariant.danger, onPressed: onDelete),
                      CcButton(label: 'Cancel', onPressed: onClose),
                      CcButton(icon: IconName.save, label: 'Save', variant: CcButtonVariant.primary, onPressed: onSave),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
