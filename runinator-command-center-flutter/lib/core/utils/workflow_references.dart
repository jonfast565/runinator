// port of core/utils/workflow-references.ts.

import '../domain/json.dart';
import '../domain/models/index.dart';
import '../workflow/workflow_helpers.dart' show workflowNodeActionConfig;

/// the data an expression editor needs to enumerate the references in scope at a given node, plus an
/// optional sample context (a prior run's data) the editor can resolve expressions against.
class WorkflowExpressionEditorContext {
  const WorkflowExpressionEditorContext({
    this.workflowInputType,
    this.nodes,
    this.currentNodeId,
    this.providers,
    this.sampleContext,
  });

  final RuninatorType? workflowInputType;
  final List<JsonRecord>? nodes;
  final String? currentNodeId;
  final List<ProviderMetadata>? providers;
  final JsonRecord? sampleContext;
}

/// a single insertable reference: what to show, the WDL text to splice in, and its declared type.
class WorkflowReference {
  const WorkflowReference({required this.label, required this.insert, required this.type});

  final String label;
  final String insert;
  final String type;
}

class NodeOutputReference extends WorkflowReference {
  const NodeOutputReference({required super.label, required super.insert, required super.type, required this.node});

  final String node;
}

/// references sharing an origin (workflow parameters, a prior node's output, or the run roots).
class ReferenceGroup {
  const ReferenceGroup({required this.title, required this.references});

  final String title;
  final List<WorkflowReference> references;
}

/// the always-available reference roots, independent of schema or prior nodes.
final List<WorkflowReference> _staticRoots = [
  const WorkflowReference(label: 'prev', insert: 'prev', type: 'previous node output'),
  const WorkflowReference(label: 'run', insert: 'run', type: 'workflow run state'),
  const WorkflowReference(label: 'config', insert: 'config', type: 'configuration value'),
  const WorkflowReference(label: 'secret', insert: 'secret', type: 'secret reference'),
];

/// references for every field of the workflow parameter struct, flattened by dotted path.
List<WorkflowReference> paramsReferences(RuninatorType? ty) {
  if (ty is! RuninatorTypeStruct) {
    return [];
  }

  final references = <WorkflowReference>[];
  _collectParamFields(ty, ['params'], references);
  return references;
}

void _collectParamFields(RuninatorType ty, List<String> path, List<WorkflowReference> references) {
  if (ty is! RuninatorTypeStruct) {
    return;
  }

  for (final entry in ty.fields.entries) {
    final nextPath = [...path, entry.key];
    final dotted = nextPath.join('.');
    references.add(WorkflowReference(label: dotted, insert: dotted, type: describeType(entry.value.ty)));
    _collectParamFields(entry.value.ty, nextPath, references);
  }
}

/// references for the declared outputs of every prior action node (the current node is excluded).
List<NodeOutputReference> nodeOutputReferences([WorkflowExpressionEditorContext? context]) {
  final nodes = context?.nodes ?? const <JsonRecord>[];
  final providers = context?.providers ?? const <ProviderMetadata>[];
  final references = <NodeOutputReference>[];

  for (final node in nodes) {
    if (node['kind'] != 'action' || node['id'] == context?.currentNodeId) {
      continue;
    }

    final config = workflowNodeActionConfig(node);
    ProviderMetadata? provider;
    for (final item in providers) {
      if (item.name == config.provider) {
        provider = item;
        break;
      }
    }
    ActionMetadata? action;
    if (provider != null) {
      for (final item in provider.actions) {
        if (item.functionName == config.action) {
          action = item;
          break;
        }
      }
    }

    for (final result in action?.results ?? const <ActionResultMetadata>[]) {
      _collectTypedReferences(
        node['id'].toString(),
        [node['id'].toString(), result.name],
        [node['id'].toString(), result.name],
        result.ty,
        references,
      );
    }
  }

  return references;
}

void _collectTypedReferences(
  String node,
  List<String> labelPath,
  List<String> insertPath,
  RuninatorType? ty,
  List<NodeOutputReference> references,
) {
  if (ty == null) {
    return;
  }

  final label = _formatLabelPath(labelPath);
  final insert = insertPath.join('.');
  references.add(NodeOutputReference(node: node, label: label, insert: insert, type: describeType(ty)));

  if (ty is RuninatorTypeStruct) {
    for (final entry in ty.fields.entries) {
      _collectTypedReferences(node, [...labelPath, entry.key], [...insertPath, entry.key], entry.value.ty, references);
    }

    return;
  }

  if (ty is RuninatorTypeArray) {
    _collectTypedReferences(node, [...labelPath, '[]'], [...insertPath, '0'], ty.items, references);
  }
}

String _formatLabelPath(List<String> parts) {
  var label = '';

  for (final part in parts) {
    if (part == '[]') {
      label += '[]';
      continue;
    }

    label = label.isNotEmpty ? '$label.$part' : part;
  }

  return label;
}

/// the full reference catalog for the picker, grouped by origin. empty groups are dropped.
List<ReferenceGroup> workflowReferenceGroups([WorkflowExpressionEditorContext? context]) {
  final groups = <ReferenceGroup>[];

  final params = paramsReferences(context?.workflowInputType);

  if (params.isNotEmpty) {
    groups.add(ReferenceGroup(title: 'Workflow parameters', references: params));
  }

  // group prior node outputs under each producing node so the source is obvious.
  final byNode = <String, List<WorkflowReference>>{};

  for (final ref in nodeOutputReferences(context)) {
    (byNode[ref.node] ??= []).add(WorkflowReference(label: ref.label, insert: ref.insert, type: ref.type));
  }

  for (final entry in byNode.entries) {
    groups.add(ReferenceGroup(title: 'Output of ${entry.key}', references: entry.value));
  }

  groups.add(ReferenceGroup(title: 'Run state', references: _staticRoots));
  return groups;
}

/// build the context a lowered expression resolves against from a run's data, mirroring the
/// reducer's runtime context: `params` is the run parameters, `steps.<node>.output` each node's
/// output, and `prev` the most recent output. `config`/`secret` are not available client-side, so
/// references to them resolve to null in a preview.
JsonRecord? buildSampleContext(WorkflowRunDetail? detail) {
  if (detail == null) {
    return null;
  }

  final steps = <String, Object?>{};
  JsonValue prev;
  Object? lastPrev;

  for (final node in detail.nodes) {
    if (node.outputJson == null) {
      continue;
    }

    steps[node.nodeId] = {'output': node.outputJson};
    lastPrev = node.outputJson;
  }

  prev = lastPrev;

  return {
    'params': detail.run.parameters ?? <String, Object?>{},
    'steps': steps,
    'prev': prev,
    'workflow': {
      'run_id': detail.run.id,
      'workflow_id': detail.run.workflowId,
      'state': detail.run.status,
    },
  };
}

/// a compact, human-readable rendering of a runinator type.
String describeType(RuninatorType? ty) {
  if (ty == null) {
    return 'any';
  }

  if (ty is RuninatorTypeArray) {
    return '${describeType(ty.items)}[]';
  }

  if (ty is RuninatorTypeMap) {
    return 'map<string, ${describeType(ty.values)}>';
  }

  if (ty is RuninatorTypeUnion) {
    return ty.variants.map(describeType).join(' | ');
  }

  if (ty is RuninatorTypeEnum) {
    return 'enum[${ty.values.map((value) => value.toString()).join(", ")}]';
  }

  if (ty is RuninatorTypeRange) {
    return '${describeType(ty.base)} range ${ty.min ?? ''}..${ty.max ?? ''}';
  }

  return ty.toJson()['type'] as String;
}
