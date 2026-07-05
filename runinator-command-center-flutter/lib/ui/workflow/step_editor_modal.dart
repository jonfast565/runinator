import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/services/workflows_service.dart';
import '../../core/workflow/workflow_helpers.dart';
import '../shared/cc_widgets.dart';
import '../theme/app_theme.dart';
import 'step_editor_sections.dart';

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

    void touch(VoidCallback mutate) {
      mutate();
      host.notify();
    }

    final nodeIds = host.ensureWorkflowNodes().map((n) => n['id']?.toString() ?? '').where((id) => id.isNotEmpty).toList();
    final ctx = StepEditorSectionContext(ref: ref, notifier: notifier, host: host, editor: editor, step: step, touch: touch, nodeIds: nodeIds);

    return Material(
      color: Colors.black54,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 860, maxHeight: 760),
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
                                style: TextStyle(fontSize: 11, color: AppColors.textMuted)),
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
                                initialValue: step.kind,
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
                        ...buildStepKindSections(ctx),
                        buildTransitionsSection(ctx),
                        if (workflows.stepEditorError.isNotEmpty)
                          Padding(
                            padding: const EdgeInsets.only(top: 8),
                            child: Text(workflows.stepEditorError, style: TextStyle(color: AppColors.dangerFg)),
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
                      CcButton(label: 'Apply Step', variant: CcButtonVariant.primary, onPressed: () => editor.submitStepEditor()),
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
