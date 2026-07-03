import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/api/command_runtime.dart';
import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/dev_pack_service.dart';
import '../../core/services/workflows_service.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../shared/split_pane.dart';
import '../workflow/wdl_editor_panel.dart';

class DevView extends ConsumerStatefulWidget {
  const DevView({super.key});

  @override
  ConsumerState<DevView> createState() => _DevViewState();
}

class _DevViewState extends ConsumerState<DevView> {
  final _pathController = TextEditingController();
  var _busy = false;
  var _skipSettings = false;
  DevPackInspectResult? _inspectResult;
  DevPackTextFile? _selectedFile;
  String? _selectedFilePath;
  String? _status;
  String? _error;

  @override
  void dispose() {
    _pathController.dispose();
    super.dispose();
  }

  bool get _isDesktop => isTauriRuntime();

  Future<void> _inspectPack() async {
    final path = _pathController.text.trim();
    if (path.isEmpty || !_isDesktop) return;

    setState(() {
      _busy = true;
      _error = null;
      _status = 'Inspecting pack...';
    });

    try {
      final result = await ref.read(devPackServiceProvider).inspect(path, _skipSettings);
      setState(() {
        _inspectResult = result;
        _status = 'Pack ready: ${result.workflows.length} workflow${result.workflows.length == 1 ? '' : 's'}.';
      });
      if (_selectedFilePath == null || !result.files.any((f) => f.path == _selectedFilePath)) {
        final first = result.files.where((f) => f.kind == 'workflow').firstOrNull ?? result.files.firstOrNull;
        if (first != null) {
          await _selectFile(first.path);
        }
      }
    } catch (err) {
      setState(() {
        _error = err.toString();
        _status = 'Inspect failed.';
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _apply() async {
    final path = _pathController.text.trim();
    if (path.isEmpty || !_isDesktop) return;

    setState(() {
      _busy = true;
      _error = null;
      _status = 'Applying pack...';
    });

    try {
      final result = await ref.read(devPackServiceProvider).apply(path, _skipSettings);
      await ref.read(workflowsProvider.notifier).catalog.refreshWorkflows();
      setState(() {
        _inspectResult = DevPackInspectResult(
          path: result.path,
          files: result.files,
          workflows: result.imported.workflows.workflows,
          triggers: result.imported.workflows.triggers,
          settingsCount: result.imported.secrets?.secrets?.length ?? 0,
          settings: _inspectResult?.settings ?? const [],
        );
        _status = 'Applied ${result.imported.workflows.workflows.length} workflow(s).';
      });
    } catch (err) {
      setState(() {
        _error = err.toString();
        _status = 'Apply failed.';
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _selectFile(String path) async {
    if (!_isDesktop) return;
    setState(() => _selectedFilePath = path);
    try {
      final file = await ref.read(devPackServiceProvider).readFile(path);
      setState(() => _selectedFile = file);
    } catch (err) {
      setState(() => _error = err.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!_isDesktop) {
      return const Padding(
        padding: EdgeInsets.all(24),
        child: EmptyState(
          icon: IconName.debug,
          message: 'The Dev environment (pack inspect/apply/watch) is only available in the desktop client.',
        ),
      );
    }

    return Padding(
      padding: const EdgeInsets.all(12),
      child: SplitPane(
        initialFirstFraction: 0.28,
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: 'Dev Pack',
                actions: [
                  CcButton(icon: IconName.refresh, label: 'Inspect', dense: true, onPressed: _busy ? null : _inspectPack),
                  CcButton(icon: IconName.upload, label: 'Apply', variant: CcButtonVariant.primary, dense: true, onPressed: _busy ? null : _apply),
                ],
              ),
              Padding(
                padding: const EdgeInsets.all(12),
                child: TextField(
                  controller: _pathController,
                  decoration: const InputDecoration(labelText: 'Pack path', hintText: '/path/to/pack'),
                ),
              ),
              SwitchListTile(
                title: const Text('Skip settings on import'),
                value: _skipSettings,
                onChanged: (v) => setState(() => _skipSettings = v),
              ),
              if (_status != null) Padding(padding: const EdgeInsets.symmetric(horizontal: 12), child: Text(_status!, style: const TextStyle(fontSize: 12))),
              if (_error != null) Padding(padding: const EdgeInsets.all(12), child: Text(_error!, style: const TextStyle(color: Colors.red, fontSize: 12))),
              Expanded(
                child: _inspectResult == null
                    ? const EmptyState(message: 'Inspect a pack to browse files.')
                    : ListView(
                        children: [
                          for (final file in _inspectResult!.files)
                            ListTile(
                              selected: file.path == _selectedFilePath,
                              title: Text(file.path, style: const TextStyle(fontSize: 12)),
                              subtitle: Text('${file.kind} · ${file.sizeBytes ?? 0} bytes'),
                              onTap: () => _selectFile(file.path),
                            ),
                        ],
                      ),
              ),
            ],
          ),
        ),
        second: _selectedFile == null
            ? const EmptyState(message: 'Select a pack file to edit.')
            : WdlEditorPanel(
                value: _selectedFile!.content,
                sourcePath: _selectedFile!.path,
                onChanged: (value) async {
                  final path = _selectedFile!.path;
                  final saved = await ref.read(devPackServiceProvider).writeFile(path, value);
                  setState(() => _selectedFile = saved);
                },
              ),
      ),
    );
  }
}
