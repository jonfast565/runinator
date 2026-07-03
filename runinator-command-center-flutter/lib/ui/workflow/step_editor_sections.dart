import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/domain/json.dart';
import '../../core/domain/models/index.dart';
import '../../core/services/providers_service.dart';
import '../../core/services/workflows/editor.dart';
import '../../core/services/workflows/host.dart';
import '../../core/services/workflows/state.dart';
import '../../core/services/workflows_service.dart';
import '../../core/workflow/editor_defaults.dart';
import '../../core/workflow/workflow_helpers.dart';
import '../shared/code_editor.dart';
import '../shared/typed_parameter_editor.dart';

class StepEditorSectionContext {
  const StepEditorSectionContext({
    required this.ref,
    required this.notifier,
    required this.host,
    required this.editor,
    required this.step,
    required this.touch,
    required this.nodeIds,
  });

  final WidgetRef ref;
  final WorkflowsNotifier notifier;
  final WorkflowServiceHost host;
  final WorkflowEditorService editor;
  final StepEditorState step;
  final void Function(VoidCallback mutate) touch;
  final List<String> nodeIds;
}

List<Widget> buildStepKindSections(StepEditorSectionContext ctx) {
  return switch (ctx.step.kind) {
    'action' => [_ActionSection(ctx: ctx)],
    'approval' => [_ApprovalSection(ctx: ctx)],
    'gate' => [_GateSection(ctx: ctx)],
    'signal' => [_SignalSection(ctx: ctx)],
    'condition' => [_ConditionSection(ctx: ctx)],
    'wait' => [_WaitSection(ctx: ctx)],
    'loop' => [_LoopSection(ctx: ctx)],
    'switch' => [_SwitchSection(ctx: ctx)],
    'toggle' => [_ToggleSection(ctx: ctx)],
    'percentage' => [_PercentageSection(ctx: ctx)],
    'parallel' => [_ParallelSection(ctx: ctx)],
    'join' => [_JoinSection(ctx: ctx)],
    'try' => [_TrySection(ctx: ctx)],
    'map' => [_MapSection(ctx: ctx)],
    'race' => [_RaceSection(ctx: ctx)],
    'output' => [_OutputSection(ctx: ctx)],
    'input' => [_InputSection(ctx: ctx)],
    'config' => [_ConfigSection(ctx: ctx)],
    'subflow' => [_SubflowSection(ctx: ctx)],
    'assert' => [_AssertSection(ctx: ctx)],
    'transform' => [_TransformSection(ctx: ctx)],
    'audit' => [_AuditSection(ctx: ctx)],
    'checkpoint' => [_CheckpointSection(ctx: ctx)],
    'mutex' => [_MutexSection(ctx: ctx)],
    'throttle' => [_ThrottleSection(ctx: ctx)],
    'await_run' => [_AwaitRunSection(ctx: ctx)],
    'debounce' => [_DebounceSection(ctx: ctx)],
    'collect' => [_CollectSection(ctx: ctx)],
    'barrier' => [_BarrierSection(ctx: ctx)],
    'circuit_breaker' => [_CircuitSection(ctx: ctx)],
    'event_source' => [_EventSourceSection(ctx: ctx)],
    _ => [const SizedBox.shrink()],
  };
}

Widget buildTransitionsSection(StepEditorSectionContext ctx) {
  return _Section(
    title: 'Transitions',
    child: Column(
      children: [
        for (final key in directTransitionKeys)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: _NodePicker(
              label: key,
              value: ctx.notifier.runs.getTransition(key),
              nodeIds: ctx.nodeIds,
              onChanged: (v) => ctx.touch(() => ctx.notifier.runs.setTransition(key, v)),
            ),
          ),
        SizedBox(
          height: 100,
          child: JsonEditor(
            value: ctx.step.transitionsJson,
            onChanged: (v) => ctx.touch(() => ctx.step.transitionsJson = v),
          ),
        ),
      ],
    ),
  );
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child});
  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
        Text(title, style: const TextStyle(fontWeight: FontWeight.w700)),
        const SizedBox(height: 8),
        child,
      ]),
    );
  }
}

class _NodePicker extends StatelessWidget {
  const _NodePicker({required this.label, required this.value, required this.nodeIds, required this.onChanged});
  final String label;
  final String value;
  final List<String> nodeIds;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    return DropdownButtonFormField<String>(
      decoration: InputDecoration(labelText: label, isDense: true),
      value: value.isEmpty ? null : value,
      items: [
        const DropdownMenuItem(value: '', child: Text('(none)')),
        for (final id in nodeIds) DropdownMenuItem(value: id, child: Text(id)),
      ],
      onChanged: (v) => onChanged(v ?? ''),
    );
  }
}

class _JsonBox extends StatelessWidget {
  const _JsonBox({required this.label, required this.value, required this.onChanged, this.height = 100});
  final String label;
  final String value;
  final ValueChanged<String> onChanged;
  final double height;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600)),
        SizedBox(height: height, child: JsonEditor(value: value, onChanged: onChanged)),
      ],
    );
  }
}

class _TextBox extends StatelessWidget {
  const _TextBox({required this.label, required this.value, required this.onChanged, this.lines = 1, this.number = false});
  final String label;
  final String value;
  final ValueChanged<String> onChanged;
  final int lines;
  final bool number;

  @override
  Widget build(BuildContext context) {
    return TextField(
      decoration: InputDecoration(labelText: label),
      maxLines: lines,
      keyboardType: number ? TextInputType.number : TextInputType.text,
      controller: TextEditingController(text: value),
      onChanged: onChanged,
    );
  }
}

class _ActionSection extends ConsumerWidget {
  const _ActionSection({required this.ctx});
  final StepEditorSectionContext ctx;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final providers = ref.watch(providersProvider).providers;
    final step = ctx.step;
    final currentProvider = providers.where((p) => p.name == step.actionName).firstOrNull;
    final actions = currentProvider?.actions ?? const <ActionMetadata>[];
    final selectedAction = actions.where((a) => a.functionName == step.actionFunction).firstOrNull;
    final params = parseStepParameters(step.parametersJson);

    return _Section(
      title: 'Action',
      child: Column(
        children: [
          DropdownButtonFormField<String>(
            decoration: const InputDecoration(labelText: 'Provider'),
            value: step.actionName.isEmpty ? null : step.actionName,
            items: [for (final p in providers) DropdownMenuItem(value: p.name, child: Text(p.name))],
            onChanged: (v) => ctx.touch(() {
              step.actionName = v ?? '';
              step.actionFunction = '';
            }),
          ),
          DropdownButtonFormField<String>(
            decoration: const InputDecoration(labelText: 'Function'),
            value: step.actionFunction.isEmpty ? null : step.actionFunction,
            items: [for (final a in actions) DropdownMenuItem(value: a.functionName, child: Text(a.functionName))],
            onChanged: (v) => ctx.touch(() => step.actionFunction = v ?? ''),
          ),
          if (selectedAction != null && selectedAction.parameters.isNotEmpty)
            TypedParameterEditor(
              parameters: selectedAction.parameters,
              credentialScopes: currentProvider?.metadata.credentialScopes ?? const [],
              value: params,
              onChanged: (next) => ctx.touch(() => writeStepParameters(next, (json) => step.parametersJson = json)),
            )
          else
            KeyValueObjectEditor(
              title: 'Action parameters',
              value: params,
              onChanged: (next) => ctx.touch(() => writeStepParameters(next, (json) => step.parametersJson = json)),
            ),
          AdvancedWdlParameters(value: step.parametersJson, onChanged: (v) => ctx.touch(() => step.parametersJson = v)),
        ],
      ),
    );
  }
}

class _ApprovalSection extends StatelessWidget {
  const _ApprovalSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Approval', child: Column(children: [
    _TextBox(label: 'Approval type', value: ctx.step.approvalType, onChanged: (v) => ctx.touch(() => ctx.step.approvalType = v)),
    _TextBox(label: 'Prompt', value: ctx.step.approvalPrompt, onChanged: (v) => ctx.touch(() => ctx.step.approvalPrompt = v), lines: 3),
  ]));
}

class _GateSection extends StatelessWidget {
  const _GateSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Gate', child: Column(children: [
    DropdownButtonFormField<String>(
      decoration: const InputDecoration(labelText: 'Kind'),
      value: ctx.step.gateKind,
      items: const [
        DropdownMenuItem(value: 'manual', child: Text('manual')),
        DropdownMenuItem(value: 'condition', child: Text('condition')),
        DropdownMenuItem(value: 'schedule', child: Text('schedule')),
      ],
      onChanged: (v) => ctx.touch(() => ctx.step.gateKind = v ?? ctx.step.gateKind),
    ),
    _TextBox(label: 'Label', value: ctx.step.gateLabel, onChanged: (v) => ctx.touch(() => ctx.step.gateLabel = v)),
    _TextBox(label: 'Poll interval (s)', value: ctx.step.gatePollInterval.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.gatePollInterval = num.tryParse(v) ?? ctx.step.gatePollInterval)),
    _TextBox(label: 'Timeout (s)', value: ctx.step.gateTimeout.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.gateTimeout = num.tryParse(v) ?? ctx.step.gateTimeout)),
    _JsonBox(label: 'When (condition)', value: ctx.step.gateWhenJson, onChanged: (v) => ctx.touch(() => ctx.step.gateWhenJson = v)),
  ]));
}

class _SignalSection extends StatelessWidget {
  const _SignalSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Signal', child: _TextBox(label: 'Signal name', value: ctx.step.signalName, onChanged: (v) => ctx.touch(() => ctx.step.signalName = v)));
}

class _ConditionSection extends StatelessWidget {
  const _ConditionSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) {
    final branches = ctx.step.conditionBranches;
    return _Section(title: 'Condition', child: Column(children: [
      for (var i = 0; i < branches.length; i++)
        Card(
          child: Padding(
            padding: const EdgeInsets.all(8),
            child: Column(children: [
              _JsonBox(label: 'When', value: branches[i].whenJson, onChanged: (v) => ctx.touch(() => branches[i].whenJson = v), height: 80),
              _NodePicker(label: 'Target', value: branches[i].target, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => branches[i].target = v)),
              Align(alignment: Alignment.centerRight, child: TextButton(onPressed: () => ctx.touch(() => ctx.editor.removeConditionBranchEditor(i)), child: const Text('Remove branch'))),
            ]),
          ),
        ),
      TextButton(onPressed: () => ctx.touch(ctx.editor.addConditionBranchEditor), child: const Text('Add branch')),
      _NodePicker(label: 'Fallback', value: ctx.step.conditionFallback, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.conditionFallback = v)),
    ]));
  }
}

class _WaitSection extends StatelessWidget {
  const _WaitSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Wait', child: Column(children: [
    _TextBox(label: 'Seconds', value: ctx.step.waitSeconds.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.waitSeconds = num.tryParse(v) ?? ctx.step.waitSeconds)),
    _TextBox(label: 'Initial status', value: ctx.step.waitInitialStatus, onChanged: (v) => ctx.touch(() => ctx.step.waitInitialStatus = v)),
    _TextBox(label: 'Until status', value: ctx.step.waitUntilStatus, onChanged: (v) => ctx.touch(() => ctx.step.waitUntilStatus = v)),
    _JsonBox(label: 'Wait settings', value: ctx.step.waitJson, onChanged: (v) => ctx.touch(() => ctx.step.waitJson = v)),
  ]));
}

class _LoopSection extends StatelessWidget {
  const _LoopSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Loop', child: Column(children: [
    _JsonBox(label: 'Items', value: ctx.step.loopItemsJson, onChanged: (v) => ctx.touch(() => ctx.step.loopItemsJson = v)),
    _NodePicker(label: 'Target', value: ctx.step.loopTarget, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.loopTarget = v)),
    _TextBox(label: 'Max iterations', value: ctx.step.loopMaxIterations.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.loopMaxIterations = num.tryParse(v) ?? ctx.step.loopMaxIterations)),
  ]));
}

class _SwitchSection extends StatelessWidget {
  const _SwitchSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) {
    final cases = ctx.step.switchCases;
    return _Section(title: 'Switch', child: Column(children: [
      _JsonBox(label: 'Value', value: ctx.step.switchValueJson, onChanged: (v) => ctx.touch(() => ctx.step.switchValueJson = v), height: 60),
      for (var i = 0; i < cases.length; i++)
        Row(children: [
          Expanded(
            child: _TextBox(
              label: 'Match kind',
              value: cases[i].matchKind,
              onChanged: (v) => ctx.touch(() => cases[i] = SwitchCaseEditor(matchKind: v, matchJson: cases[i].matchJson, target: cases[i].target)),
            ),
          ),
          Expanded(
            child: _JsonBox(
              label: 'Match',
              value: cases[i].matchJson,
              onChanged: (v) => ctx.touch(() => cases[i] = SwitchCaseEditor(matchKind: cases[i].matchKind, matchJson: v, target: cases[i].target)),
              height: 60,
            ),
          ),
          Expanded(
            child: _NodePicker(
              label: 'Target',
              value: cases[i].target,
              nodeIds: ctx.nodeIds,
              onChanged: (v) => ctx.touch(() => cases[i] = SwitchCaseEditor(matchKind: cases[i].matchKind, matchJson: cases[i].matchJson, target: v)),
            ),
          ),
          IconButton(icon: const Icon(Icons.delete_outline), onPressed: () => ctx.touch(() => ctx.editor.removeSwitchCaseEditor(i))),
        ]),
      TextButton(onPressed: () => ctx.touch(ctx.editor.addSwitchCaseEditor), child: const Text('Add case')),
      _NodePicker(label: 'Default', value: ctx.step.switchDefault, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.switchDefault = v)),
    ]));
  }
}

class _ToggleSection extends StatelessWidget {
  const _ToggleSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Toggle', child: Column(children: [
    _JsonBox(label: 'Value', value: ctx.step.toggleValueJson, onChanged: (v) => ctx.touch(() => ctx.step.toggleValueJson = v), height: 60),
    _NodePicker(label: 'On', value: ctx.step.toggleOn, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.toggleOn = v)),
    _NodePicker(label: 'Off', value: ctx.step.toggleOff, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.toggleOff = v)),
  ]));
}

class _PercentageSection extends StatelessWidget {
  const _PercentageSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) {
    final buckets = ctx.step.percentageBuckets;
    return _Section(title: 'Percentage', child: Column(children: [
      _JsonBox(label: 'Key', value: ctx.step.percentageKeyJson, onChanged: (v) => ctx.touch(() => ctx.step.percentageKeyJson = v), height: 60),
      for (var i = 0; i < buckets.length; i++)
        Row(children: [
          Expanded(child: _TextBox(label: 'Weight', value: buckets[i].weight.toString(), number: true, onChanged: (v) => ctx.touch(() => buckets[i].weight = num.tryParse(v) ?? buckets[i].weight))),
          Expanded(child: _NodePicker(label: 'Target', value: buckets[i].target, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => buckets[i].target = v))),
          IconButton(icon: const Icon(Icons.delete_outline), onPressed: () => ctx.touch(() => ctx.editor.removePercentageBucketEditor(i))),
        ]),
      TextButton(onPressed: () => ctx.touch(ctx.editor.addPercentageBucketEditor), child: const Text('Add bucket')),
      _NodePicker(label: 'Default', value: ctx.step.percentageDefault, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.percentageDefault = v)),
    ]));
  }
}

class _BranchListSection extends StatelessWidget {
  const _BranchListSection({required this.title, required this.branches, required this.ctx});
  final String title;
  final List<String> branches;
  final StepEditorSectionContext ctx;

  @override
  Widget build(BuildContext context) {
    return _Section(title: title, child: Column(children: [
      for (var i = 0; i < branches.length; i++)
        Row(children: [
          Expanded(child: _NodePicker(label: 'Branch ${i + 1}', value: branches[i], nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => branches[i] = v))),
          IconButton(icon: const Icon(Icons.delete_outline), onPressed: () => ctx.touch(() => ctx.editor.removeNodeRefEditor(branches, i))),
        ]),
      TextButton(onPressed: () => ctx.touch(() => ctx.editor.addNodeRefEditor(branches)), child: const Text('Add branch')),
    ]));
  }
}

class _ParallelSection extends StatelessWidget {
  const _ParallelSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _BranchListSection(title: 'Parallel', branches: ctx.step.parallelBranches, ctx: ctx);
}

class _JoinSection extends StatelessWidget {
  const _JoinSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => Column(children: [
    _BranchListSection(title: 'Join — wait for', branches: ctx.step.joinWaitFor, ctx: ctx),
    _Section(title: 'Join', child: _TextBox(label: 'Mode', value: ctx.step.joinMode, onChanged: (v) => ctx.touch(() => ctx.step.joinMode = v))),
  ]);
}

class _TrySection extends StatelessWidget {
  const _TrySection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Try', child: Column(children: [
    _NodePicker(label: 'Body', value: ctx.step.tryBody, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.tryBody = v)),
    _NodePicker(label: 'Catch', value: ctx.step.tryCatch, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.tryCatch = v)),
    _NodePicker(label: 'Finally', value: ctx.step.tryFinally, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.tryFinally = v)),
  ]));
}

class _MapSection extends StatelessWidget {
  const _MapSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Map', child: Column(children: [
    _JsonBox(label: 'Items', value: ctx.step.mapItemsJson, onChanged: (v) => ctx.touch(() => ctx.step.mapItemsJson = v)),
    _NodePicker(label: 'Target', value: ctx.step.mapTarget, nodeIds: ctx.nodeIds, onChanged: (v) => ctx.touch(() => ctx.step.mapTarget = v)),
    _TextBox(label: 'Concurrency', value: ctx.step.mapConcurrency.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.mapConcurrency = num.tryParse(v) ?? ctx.step.mapConcurrency)),
  ]));
}

class _RaceSection extends StatelessWidget {
  const _RaceSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Race', child: Column(children: [
    _BranchListSection(title: 'Branches', branches: ctx.step.raceBranches, ctx: ctx),
    _TextBox(label: 'Winner', value: ctx.step.raceWinner, onChanged: (v) => ctx.touch(() => ctx.step.raceWinner = v)),
  ]));
}

class _OutputSection extends StatelessWidget {
  const _OutputSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Output', child: Column(children: [
    _TextBox(label: 'Event type', value: ctx.step.outputEventType, onChanged: (v) => ctx.touch(() => ctx.step.outputEventType = v)),
    _JsonBox(label: 'Data', value: ctx.step.outputDataJson, onChanged: (v) => ctx.touch(() => ctx.step.outputDataJson = v)),
  ]));
}

class _InputSection extends StatelessWidget {
  const _InputSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Input', child: _TextBox(label: 'Prompt', value: ctx.step.inputPrompt, onChanged: (v) => ctx.touch(() => ctx.step.inputPrompt = v), lines: 2));
}

class _ConfigSection extends StatelessWidget {
  const _ConfigSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Config', child: Column(children: [
    _JsonBox(label: 'Name', value: ctx.step.configNameJson, onChanged: (v) => ctx.touch(() => ctx.step.configNameJson = v), height: 60),
    _JsonBox(label: 'Metadata', value: ctx.step.configMetadataJson, onChanged: (v) => ctx.touch(() => ctx.step.configMetadataJson = v)),
  ]));
}

class _SubflowSection extends StatelessWidget {
  const _SubflowSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Subflow', child: Column(children: [
    _TextBox(label: 'Subflow ID', value: ctx.step.subflowId, onChanged: (v) => ctx.touch(() => ctx.step.subflowId = v)),
    _JsonBox(label: 'Parameters', value: ctx.step.subflowParametersJson, onChanged: (v) => ctx.touch(() => ctx.step.subflowParametersJson = v)),
  ]));
}

class _AssertSection extends StatelessWidget {
  const _AssertSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) {
    final assertions = ctx.step.assertAssertions;
    return _Section(title: 'Assert', child: Column(children: [
      for (var i = 0; i < assertions.length; i++)
        Card(child: Padding(padding: const EdgeInsets.all(8), child: Column(children: [
          _TextBox(label: 'Name', value: assertions[i].name, onChanged: (v) => ctx.touch(() => assertions[i].name = v)),
          _JsonBox(label: 'Condition', value: assertions[i].conditionJson, onChanged: (v) => ctx.touch(() => assertions[i].conditionJson = v), height: 70),
          _TextBox(label: 'Message', value: assertions[i].message, onChanged: (v) => ctx.touch(() => assertions[i].message = v)),
          TextButton(onPressed: () => ctx.touch(() => ctx.editor.removeAssertionEditor(i)), child: const Text('Remove')),
        ]))),
      TextButton(onPressed: () => ctx.touch(ctx.editor.addAssertionEditor), child: const Text('Add assertion')),
    ]));
  }
}

class _TransformSection extends StatelessWidget {
  const _TransformSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Transform', child: _JsonBox(label: 'Bindings', value: ctx.step.transformBindingsJson, onChanged: (v) => ctx.touch(() => ctx.step.transformBindingsJson = v)));
}

class _AuditSection extends StatelessWidget {
  const _AuditSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Audit', child: Column(children: [
    _JsonBox(label: 'Action', value: ctx.step.auditActionJson, onChanged: (v) => ctx.touch(() => ctx.step.auditActionJson = v), height: 60),
    _TextBox(label: 'Actor', value: ctx.step.auditActorJson, onChanged: (v) => ctx.touch(() => ctx.step.auditActorJson = v)),
    _TextBox(label: 'Target', value: ctx.step.auditTargetJson, onChanged: (v) => ctx.touch(() => ctx.step.auditTargetJson = v)),
    _TextBox(label: 'Reason', value: ctx.step.auditReasonJson, onChanged: (v) => ctx.touch(() => ctx.step.auditReasonJson = v)),
  ]));
}

class _CheckpointSection extends StatelessWidget {
  const _CheckpointSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Checkpoint', child: _TextBox(label: 'Name', value: ctx.step.checkpointName, onChanged: (v) => ctx.touch(() => ctx.step.checkpointName = v)));
}

class _MutexSection extends StatelessWidget {
  const _MutexSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Mutex', child: Column(children: [
    _TextBox(label: 'Name', value: ctx.step.mutexName, onChanged: (v) => ctx.touch(() => ctx.step.mutexName = v)),
    _TextBox(label: 'Poll interval', value: ctx.step.mutexPollInterval.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.mutexPollInterval = num.tryParse(v) ?? ctx.step.mutexPollInterval)),
  ]));
}

class _ThrottleSection extends StatelessWidget {
  const _ThrottleSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Throttle', child: Column(children: [
    _TextBox(label: 'Name', value: ctx.step.throttleName, onChanged: (v) => ctx.touch(() => ctx.step.throttleName = v)),
    _TextBox(label: 'Max per window', value: ctx.step.throttleMaxPerWindow.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.throttleMaxPerWindow = num.tryParse(v) ?? ctx.step.throttleMaxPerWindow)),
    _TextBox(label: 'Window seconds', value: ctx.step.throttleWindowSeconds.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.throttleWindowSeconds = num.tryParse(v) ?? ctx.step.throttleWindowSeconds)),
    _TextBox(label: 'Poll interval', value: ctx.step.throttlePollInterval.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.throttlePollInterval = num.tryParse(v) ?? ctx.step.throttlePollInterval)),
  ]));
}

class _AwaitRunSection extends StatelessWidget {
  const _AwaitRunSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Await run', child: Column(children: [
    _JsonBox(label: 'Run IDs', value: ctx.step.awaitRunIdsJson, onChanged: (v) => ctx.touch(() => ctx.step.awaitRunIdsJson = v), height: 70),
    _TextBox(label: 'Mode', value: ctx.step.awaitMode, onChanged: (v) => ctx.touch(() => ctx.step.awaitMode = v)),
    _TextBox(label: 'Poll interval', value: ctx.step.awaitPollInterval.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.awaitPollInterval = num.tryParse(v) ?? ctx.step.awaitPollInterval)),
  ]));
}

class _DebounceSection extends StatelessWidget {
  const _DebounceSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Debounce', child: Column(children: [
    _TextBox(label: 'Name', value: ctx.step.debounceName, onChanged: (v) => ctx.touch(() => ctx.step.debounceName = v)),
    _TextBox(label: 'Delay seconds', value: ctx.step.debounceDelaySeconds.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.debounceDelaySeconds = num.tryParse(v) ?? ctx.step.debounceDelaySeconds)),
    _JsonBox(label: 'Trigger key', value: ctx.step.debounceTriggerKeyJson, onChanged: (v) => ctx.touch(() => ctx.step.debounceTriggerKeyJson = v), height: 60),
  ]));
}

class _CollectSection extends StatelessWidget {
  const _CollectSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Collect', child: Column(children: [
    _TextBox(label: 'Name', value: ctx.step.collectName, onChanged: (v) => ctx.touch(() => ctx.step.collectName = v)),
    _TextBox(label: 'Max', value: ctx.step.collectMax.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.collectMax = num.tryParse(v) ?? ctx.step.collectMax)),
  ]));
}

class _BarrierSection extends StatelessWidget {
  const _BarrierSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Barrier', child: Column(children: [
    _TextBox(label: 'Name', value: ctx.step.barrierName, onChanged: (v) => ctx.touch(() => ctx.step.barrierName = v)),
    _TextBox(label: 'Count', value: ctx.step.barrierCount.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.barrierCount = num.tryParse(v) ?? ctx.step.barrierCount)),
    _TextBox(label: 'Poll interval', value: ctx.step.barrierPollInterval.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.barrierPollInterval = num.tryParse(v) ?? ctx.step.barrierPollInterval)),
  ]));
}

class _CircuitSection extends StatelessWidget {
  const _CircuitSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Circuit breaker', child: Column(children: [
    _TextBox(label: 'Name', value: ctx.step.circuitName, onChanged: (v) => ctx.touch(() => ctx.step.circuitName = v)),
    _TextBox(label: 'Threshold', value: ctx.step.circuitThreshold.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.circuitThreshold = num.tryParse(v) ?? ctx.step.circuitThreshold)),
    _TextBox(label: 'Window seconds', value: ctx.step.circuitWindowSeconds.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.circuitWindowSeconds = num.tryParse(v) ?? ctx.step.circuitWindowSeconds)),
    _TextBox(label: 'Cooldown seconds', value: ctx.step.circuitCooldownSeconds.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.circuitCooldownSeconds = num.tryParse(v) ?? ctx.step.circuitCooldownSeconds)),
  ]));
}

class _EventSourceSection extends StatelessWidget {
  const _EventSourceSection({required this.ctx});
  final StepEditorSectionContext ctx;
  @override
  Widget build(BuildContext context) => _Section(title: 'Event source', child: Column(children: [
    _TextBox(label: 'Type', value: ctx.step.eventSourceType, onChanged: (v) => ctx.touch(() => ctx.step.eventSourceType = v)),
    _JsonBox(label: 'Filter', value: ctx.step.eventSourceFilterJson, onChanged: (v) => ctx.touch(() => ctx.step.eventSourceFilterJson = v), height: 70),
    _TextBox(label: 'Max events (0 = unlimited)', value: ctx.step.eventSourceMax.toString(), number: true, onChanged: (v) => ctx.touch(() => ctx.step.eventSourceMax = num.tryParse(v) ?? ctx.step.eventSourceMax)),
  ]));
}
