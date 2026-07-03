import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/models/index.dart';
import '../../core/services/providers_service.dart';
import '../../core/services/workflows_service.dart';
import '../../core/workflow/workflow_helpers.dart';
import '../shared/cc_widgets.dart';
import '../shared/code_editor.dart';
import '../theme/app_theme.dart';

class StepEditorModal extends ConsumerWidget {
  const StepEditorModal({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final workflows = ref.watch(workflowsProvider);
    if (!workflows.stepEditorOpen) return const SizedBox.shrink();

    final notifier = ref.read(workflowsProvider.notifier);
    final host = notifier.host;
    final editor = notifier.editor;
    final step = workflows.stepEditor;
    final providers = ref.watch(providersProvider).providers;

    void touch(VoidCallback mutate) {
      mutate();
      host.notify();
    }

    final currentProvider = providers.where((p) => p.name == step.actionName).firstOrNull;
    final actions = currentProvider?.actions ?? const <ActionMetadata>[];

    return Material(
      color: Colors.black54,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 820, maxHeight: 720),
          child: Card(
            margin: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 16, 8, 0),
                  child: Row(
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              workflows.stepEditorCreating ? 'Add Workflow Step' : 'Edit Workflow Step',
                              style: const TextStyle(fontWeight: FontWeight.w700, fontSize: 16),
                            ),
                            Text(workflows.selectedStepId.isEmpty ? 'New step' : workflows.selectedStepId,
                                style: const TextStyle(fontSize: 11, color: AppColors.textMuted)),
                          ],
                        ),
                      ),
                      IconButton(icon: const Icon(Icons.close), onPressed: () => editor.closeStepEditor()),
                    ],
                  ),
                ),
                Expanded(
                  child: SingleChildScrollView(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        _Section(
                          title: 'Step',
                          child: Column(
                            children: [
                              TextField(
                                decoration: const InputDecoration(labelText: 'Step ID'),
                                controller: TextEditingController(text: step.id),
                                onChanged: (v) => touch(() => step.id = v),
                              ),
                              const SizedBox(height: 8),
                              TextField(
                                decoration: const InputDecoration(labelText: 'Name'),
                                controller: TextEditingController(text: step.name),
                                onChanged: (v) => touch(() => step.name = v),
                              ),
                              const SizedBox(height: 8),
                              DropdownButtonFormField<String>(
                                decoration: const InputDecoration(labelText: 'Node Kind'),
                                value: step.kind,
                                items: [
                                  const DropdownMenuItem(value: 'start', child: Text('start')),
                                  for (final kind in notifier.nodeKinds)
                                    DropdownMenuItem(value: kind, child: Text(workflowNodeKindLabel(kind))),
                                  const DropdownMenuItem(value: 'end', child: Text('end')),
                                  const DropdownMenuItem(value: 'fail', child: Text('fail')),
                                ],
                                onChanged: (v) {
                                  if (v != null) touch(() => step.kind = v);
                                },
                              ),
                            ],
                          ),
                        ),
                        _Section(
                          title: 'Runtime',
                          child: Column(
                            children: [
                              TextField(
                                decoration: const InputDecoration(labelText: 'Max attempts'),
                                keyboardType: TextInputType.number,
                                controller: TextEditingController(text: step.maxAttempts.toString()),
                                onChanged: (v) => touch(() => step.maxAttempts = num.tryParse(v) ?? step.maxAttempts),
                              ),
                              const SizedBox(height: 8),
                              TextField(
                                decoration: const InputDecoration(labelText: 'Timeout seconds (0 = default)'),
                                keyboardType: TextInputType.number,
                                controller: TextEditingController(text: step.timeoutSeconds.toString()),
                                onChanged: (v) => touch(() => step.timeoutSeconds = num.tryParse(v) ?? step.timeoutSeconds),
                              ),
                              SwitchListTile(
                                title: const Text('Locked'),
                                value: step.locked,
                                onChanged: (v) => touch(() => step.locked = v),
                              ),
                              SwitchListTile(
                                title: const Text('Skipped'),
                                value: step.skipped,
                                onChanged: (v) => touch(() => step.skipped = v),
                              ),
                            ],
                          ),
                        ),
                        if (step.kind == 'action') ...[
                          _Section(
                            title: 'Action',
                            child: Column(
                              children: [
                                DropdownButtonFormField<String>(
                                  decoration: const InputDecoration(labelText: 'Provider'),
                                  value: step.actionName.isEmpty ? null : step.actionName,
                                  items: [
                                    for (final provider in providers)
                                      DropdownMenuItem(value: provider.name, child: Text(provider.name)),
                                  ],
                                  onChanged: (v) => touch(() {
                                    step.actionName = v ?? '';
                                    step.actionFunction = '';
                                  }),
                                ),
                                const SizedBox(height: 8),
                                DropdownButtonFormField<String>(
                                  decoration: const InputDecoration(labelText: 'Function'),
                                  value: step.actionFunction.isEmpty ? null : step.actionFunction,
                                  items: [
                                    for (final action in actions)
                                      DropdownMenuItem(value: action.functionName, child: Text(action.functionName)),
                                  ],
                                  onChanged: (v) => touch(() => step.actionFunction = v ?? ''),
                                ),
                                const SizedBox(height: 8),
                                SizedBox(
                                  height: 160,
                                  child: JsonEditor(
                                    value: step.parametersJson,
                                    onChanged: (v) => touch(() => step.parametersJson = v),
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ],
                        if (step.kind == 'approval')
                          _Section(
                            title: 'Approval',
                            child: Column(
                              children: [
                                TextField(
                                  decoration: const InputDecoration(labelText: 'Approval type'),
                                  controller: TextEditingController(text: step.approvalType),
                                  onChanged: (v) => touch(() => step.approvalType = v),
                                ),
                                TextField(
                                  decoration: const InputDecoration(labelText: 'Prompt'),
                                  maxLines: 3,
                                  controller: TextEditingController(text: step.approvalPrompt),
                                  onChanged: (v) => touch(() => step.approvalPrompt = v),
                                ),
                              ],
                            ),
                          ),
                        if (step.kind == 'gate')
                          _Section(
                            title: 'Gate',
                            child: Column(
                              children: [
                                TextField(
                                  decoration: const InputDecoration(labelText: 'Kind'),
                                  controller: TextEditingController(text: step.gateKind),
                                  onChanged: (v) => touch(() => step.gateKind = v),
                                ),
                                TextField(
                                  decoration: const InputDecoration(labelText: 'Label'),
                                  controller: TextEditingController(text: step.gateLabel),
                                  onChanged: (v) => touch(() => step.gateLabel = v),
                                ),
                                SizedBox(
                                  height: 100,
                                  child: JsonEditor(
                                    value: step.gateWhenJson,
                                    onChanged: (v) => touch(() => step.gateWhenJson = v),
                                  ),
                                ),
                              ],
                            ),
                          ),
                        if (step.kind == 'wait')
                          _Section(
                            title: 'Wait',
                            child: Column(
                              children: [
                                TextField(
                                  decoration: const InputDecoration(labelText: 'Seconds'),
                                  keyboardType: TextInputType.number,
                                  controller: TextEditingController(text: step.waitSeconds.toString()),
                                  onChanged: (v) => touch(() => step.waitSeconds = num.tryParse(v) ?? step.waitSeconds),
                                ),
                                SizedBox(
                                  height: 100,
                                  child: JsonEditor(
                                    value: step.waitJson,
                                    onChanged: (v) => touch(() => step.waitJson = v),
                                  ),
                                ),
                              ],
                            ),
                          ),
                        if (step.kind == 'subflow')
                          _Section(
                            title: 'Subflow',
                            child: Column(
                              children: [
                                TextField(
                                  decoration: const InputDecoration(labelText: 'Subflow ID'),
                                  controller: TextEditingController(text: step.subflowId),
                                  onChanged: (v) => touch(() => step.subflowId = v),
                                ),
                                SizedBox(
                                  height: 120,
                                  child: JsonEditor(
                                    value: step.subflowParametersJson,
                                    onChanged: (v) => touch(() => step.subflowParametersJson = v),
                                  ),
                                ),
                              ],
                            ),
                          ),
                        _Section(
                          title: 'Transitions',
                          child: SizedBox(
                            height: 120,
                            child: JsonEditor(
                              value: step.transitionsJson,
                              onChanged: (v) => touch(() => step.transitionsJson = v),
                            ),
                          ),
                        ),
                        if (workflows.stepEditorError.isNotEmpty)
                          Padding(
                            padding: const EdgeInsets.only(top: 8),
                            child: Text(workflows.stepEditorError, style: const TextStyle(color: AppColors.dangerFg)),
                          ),
                      ],
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.all(16),
                  child: Wrap(
                    spacing: 8,
                    children: [
                      CcButton(label: 'Cancel', onPressed: () => editor.closeStepEditor()),
                      CcButton(
                        label: 'Apply Step',
                        variant: CcButtonVariant.primary,
                        onPressed: () => editor.submitStepEditor(),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child});

  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(title, style: const TextStyle(fontWeight: FontWeight.w700)),
          const SizedBox(height: 8),
          child,
        ],
      ),
    );
  }
}
