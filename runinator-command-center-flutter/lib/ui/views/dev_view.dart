import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/api/command_runtime.dart';
import '../../core/domain/icons.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/dev_pack_service.dart';
import '../../core/services/workflows_service.dart';
import '../../core/utils/format.dart';
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
  var _autoInspect = true;
  var _autoApply = false;
  var _autoSave = true;
  var _debugRun = false;
  var _runAfterApply = false;
  var _runWorkflowRef = '';
  DevPackInspectResult? _inspectResult;
  DevPackTextFile? _selectedFile;
  String? _selectedFilePath;
  String? _status;
  String? _error;
  String? _latestRunId;
  WorkflowRunDetail? _latestRunDetail;
  Timer? _inspectTimer;
  Timer? _runTimer;
  Timer? _autoSaveTimer;
  String? _lastFingerprint;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_isDesktop) {
        _inspectTimer = Timer.periodic(const Duration(milliseconds: 1500), (_) {
          if (_autoInspect && !_busy) {
            _inspectPack(quiet: true, applyOnChange: _autoApply);
          }
        });
      }
    });
  }

  @override
  void dispose() {
    _inspectTimer?.cancel();
    _runTimer?.cancel();
    _autoSaveTimer?.cancel();
    _pathController.dispose();
    super.dispose();
  }

  bool get _isDesktop => isTauriRuntime();

  String _fingerprint(List<DevPackFile> files) => jsonEncode(files.map((f) => '${f.path}:${f.modifiedAt}:${f.sizeBytes}').toList());

  Future<void> _inspectPack({bool quiet = false, bool applyOnChange = false}) async {
    final path = _pathController.text.trim();
    if (path.isEmpty || !_isDesktop) return;

    if (!quiet) {
      setState(() {
        _busy = true;
        _error = null;
        _status = 'Inspecting pack...';
      });
    }

    try {
      final previous = _lastFingerprint;
      final result = await ref.read(devPackServiceProvider).inspect(path, _skipSettings);
      final fingerprint = _fingerprint(result.files);
      setState(() {
        _inspectResult = result;
        if (!quiet) _status = 'Pack ready: ${result.workflows.length} workflow(s).';
      });
      _lastFingerprint = fingerprint;

      if (_selectedFilePath == null || !result.files.any((f) => f.path == _selectedFilePath)) {
        final first = result.files.where((f) => f.kind == 'workflow').firstOrNull ?? result.files.firstOrNull;
        if (first != null) await _selectFile(first.path);
      } else if (previous != null && previous != fingerprint) {
        await _reloadSelectedSource();
      }

      if (applyOnChange && previous != null && previous != fingerprint) {
        await _apply(quiet: true);
      }
    } catch (err) {
      if (!quiet) {
        setState(() {
          _error = err.toString();
          _status = 'Inspect failed.';
        });
      }
    } finally {
      if (!quiet && mounted) setState(() => _busy = false);
    }
  }

  Future<void> _apply({bool quiet = false}) async {
    final path = _pathController.text.trim();
    if (path.isEmpty || !_isDesktop) return;

    if (!quiet) {
      setState(() {
        _busy = true;
        _error = null;
        _status = 'Applying pack...';
      });
    }

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
        _lastFingerprint = _fingerprint(result.files);
        if (!quiet) _status = 'Applied ${result.imported.workflows.workflows.length} workflow(s).';
      });
      if (_runAfterApply && _runWorkflowRef.isNotEmpty) {
        await _runSelectedWorkflow();
      }
    } catch (err) {
      if (!quiet) {
        setState(() {
          _error = err.toString();
          _status = 'Apply failed.';
        });
      }
    } finally {
      if (!quiet && mounted) setState(() => _busy = false);
    }
  }

  Future<void> _selectFile(String path) async {
    if (!_isDesktop) return;
    setState(() => _selectedFilePath = path);
    await _reloadSelectedSource();
  }

  Future<void> _reloadSelectedSource() async {
    final path = _selectedFilePath;
    if (path == null) return;
    try {
      final file = await ref.read(devPackServiceProvider).readFile(path);
      if (mounted) setState(() => _selectedFile = file);
    } catch (err) {
      if (mounted) setState(() => _error = err.toString());
    }
  }

  void _scheduleAutoSave(String contents) {
    if (!_autoSave || _selectedFilePath == null) return;
    _autoSaveTimer?.cancel();
    _autoSaveTimer = Timer(const Duration(milliseconds: 800), () async {
      final path = _selectedFilePath;
      if (path == null) return;
      try {
        await ref.read(devPackServiceProvider).writeFile(path, contents);
        await _inspectPack(quiet: true, applyOnChange: _autoApply);
      } catch (err) {
        if (mounted) setState(() => _error = err.toString());
      }
    });
  }

  WorkflowDefinition? _resolveRunWorkflow() {
    final value = _runWorkflowRef.trim();
    if (value.isEmpty) return null;
    for (final workflow in _inspectResult?.workflows ?? const <WorkflowDefinition>[]) {
      if (workflow.id == value || workflow.name == value) return workflow;
    }
    for (final workflow in ref.read(workflowsProvider).workflows) {
      if (workflow.id == value || workflow.name == value) return workflow;
    }
    return null;
  }

  Future<void> _runSelectedWorkflow() async {
    final workflow = _resolveRunWorkflow();
    if (workflow?.id == null) {
      setState(() => _error = 'Workflow not found: $_runWorkflowRef');
      return;
    }
    try {
      final created = await ref.read(devPackServiceProvider).createRun(workflow!.id!, debug: _debugRun);
      setState(() {
        _latestRunId = created.id;
        _status = 'Started workflow run #${created.id}.';
      });
      await _refreshLatestRun();
      _watchLatestRun();
    } catch (err) {
      setState(() => _error = err.toString());
    }
  }

  Future<void> _refreshLatestRun() async {
    final runId = _latestRunId;
    if (runId == null) return;
    try {
      final detail = await ref.read(devPackServiceProvider).fetchRun(runId);
      if (mounted) setState(() => _latestRunDetail = detail);
    } catch (_) {}
  }

  void _watchLatestRun() {
    _runTimer?.cancel();
    _runTimer = Timer.periodic(const Duration(milliseconds: 1500), (_) async {
      await _refreshLatestRun();
      final status = _latestRunDetail?.run.status;
      if (status != null && const {'succeeded', 'failed', 'canceled', 'timed_out'}.contains(status)) {
        _runTimer?.cancel();
      }
    });
  }

  Widget _buildSourceEditor() {
    final file = _selectedFile;
    if (file == null) return const EmptyState(message: 'Select a pack file to edit.');
    final isWdl = file.path.endsWith('.wdl') || file.path.endsWith('.wdlp');
    if (isWdl) {
      return WdlEditorPanel(
        value: file.content,
        sourcePath: file.path,
        onChanged: (value) {
          setState(() => _selectedFile = DevPackTextFile(path: file.path, content: value, modifiedAt: file.modifiedAt));
          _scheduleAutoSave(value);
        },
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(file.path, style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 12)),
        const SizedBox(height: 8),
        Expanded(
          child: file.path.endsWith('.json')
              ? JsonEditor(value: file.content, onChanged: (v) => _scheduleAutoSave(v))
              : CodeEditor(value: file.content, onChanged: (v) => _scheduleAutoSave(v), minLines: 20),
        ),
      ],
    );
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
        initialFirstFraction: 0.24,
        first: PanelCard(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelToolbar(
                title: 'Dev Pack',
                actions: [
                  CcButton(icon: IconName.refresh, label: 'Inspect', dense: true, onPressed: _busy ? null : () => _inspectPack()),
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
              SwitchListTile(title: const Text('Skip settings'), value: _skipSettings, onChanged: (v) => setState(() => _skipSettings = v)),
              SwitchListTile(title: const Text('Auto inspect'), value: _autoInspect, onChanged: (v) => setState(() => _autoInspect = v)),
              SwitchListTile(title: const Text('Auto apply on change'), value: _autoApply, onChanged: (v) => setState(() => _autoApply = v)),
              SwitchListTile(title: const Text('Auto save edits'), value: _autoSave, onChanged: (v) => setState(() => _autoSave = v)),
              SwitchListTile(title: const Text('Debug run'), value: _debugRun, onChanged: (v) => setState(() => _debugRun = v)),
              SwitchListTile(title: const Text('Run after apply'), value: _runAfterApply, onChanged: (v) => setState(() => _runAfterApply = v)),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12),
                child: TextField(
                  decoration: const InputDecoration(labelText: 'Run workflow ref', hintText: 'workflow name or id'),
                  controller: TextEditingController(text: _runWorkflowRef),
                  onChanged: (v) => _runWorkflowRef = v,
                ),
              ),
              Padding(
                padding: const EdgeInsets.all(12),
                child: CcButton(icon: IconName.play, label: 'Run workflow', dense: true, onPressed: _runSelectedWorkflow),
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
        second: SplitPane(
          initialFirstFraction: 0.62,
          first: _buildSourceEditor(),
          second: PanelCard(
            child: _latestRunDetail == null
                ? const EmptyState(message: 'Latest run appears here after you start one.')
                : SingleChildScrollView(
                    padding: const EdgeInsets.all(12),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Run #${_latestRunDetail!.run.id}', style: const TextStyle(fontWeight: FontWeight.w700)),
                        Text('Status: ${_latestRunDetail!.run.status}', style: const TextStyle(fontSize: 12)),
                        const SizedBox(height: 12),
                        for (final node in _latestRunDetail!.nodes)
                          ListTile(
                            dense: true,
                            title: Text('${node.nodeId} · ${node.status}'),
                            subtitle: Text(node.message ?? '', style: const TextStyle(fontSize: 10)),
                          ),
                        const SizedBox(height: 12),
                        SizedBox(
                          height: 180,
                          child: JsonEditor(value: pretty(_latestRunDetail!.run.outputJson), onChanged: (_) {}, readOnly: true),
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
