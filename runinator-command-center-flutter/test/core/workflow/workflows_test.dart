import 'dart:convert';

import 'package:runinator_command_center_flutter/core/domain/json.dart';
import 'package:runinator_command_center_flutter/core/domain/models/index.dart';
import 'package:runinator_command_center_flutter/core/domain/models/provider/action_metadata.dart';
import 'package:runinator_command_center_flutter/core/domain/models/provider/provider_metadata.dart';
import 'package:runinator_command_center_flutter/core/domain/models/provider/runinator_type.dart';
import 'package:runinator_command_center_flutter/core/domain/models/run/run_summary.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow/edge.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow/transitions.dart';
import 'package:runinator_command_center_flutter/core/utils/json_utils.dart';
import 'package:runinator_command_center_flutter/core/workflow/editor_defaults.dart';
import 'package:runinator_command_center_flutter/core/workflow/graph_model.dart';
import 'package:runinator_command_center_flutter/core/workflow/workflow_helpers.dart';
import 'package:test/test.dart';

const workflowId = '00000000-0000-0000-0000-000000000001';
const runId = '00000000-0000-0000-0000-000000000010';
const nodeRunId = '00000000-0000-0000-0000-000000000011';
const searchWorkflowId = '00000000-0000-0000-0000-000000000034';
const searchRunId = '00000000-0000-0000-0000-000000000012';

JsonRecord looseMap(Object value) => asJsonRecord(cloneJson(value));

WorkflowDefinition _cloneWorkflow(WorkflowDefinition workflow) =>
    WorkflowDefinition.fromJson(workflow.toJson());

WorkflowDefinition workflowFixture() => WorkflowDefinition(
      id: workflowId,
      name: 'Flow',
      version: '1.0.0',
      enabled: true,
      inputType: {'type': 'any'},
      definition: looseMap({
        'nodes': [
          {
            'id': 'a',
            'kind': 'action',
            'action': {'provider': 'Console', 'function': 'run', 'configuration': {}},
            'transitions': {'next': {r'$node': 'b'}},
          },
          {
            'id': 'b',
            'kind': 'action',
            'action': {'provider': 'Console', 'function': 'run', 'configuration': {}},
            'transitions': {},
          },
        ],
        'ui': {
          'layout': {
            'nodes': {'a': {'x': 10, 'y': 20}},
          },
        },
      }),
    );

void main() {
  group('workflow graph utils', () {
    late WorkflowDefinition workflow;

    setUp(() {
      workflow = workflowFixture();
    });

    test('builds positioned graph nodes', () {
      final nodes = buildGraphNodeModels(workflow, null);
      expect(nodes[0].position.x, 10);
      expect(nodes[0].position.y, 20);
    });

    test('does not add status classes without run detail', () {
      final nodes = buildGraphNodeModels(workflow, null);
      expect(nodes.every((node) => node.className == ''), isTrue);
      expect(nodes.every((node) => node.data.status == null), isTrue);
    });

    test('summarizes imported action nodes from embedded action configuration', () {
      final nodes = buildGraphNodeModels(
        WorkflowDefinition(
          id: workflow.id,
          name: workflow.name,
          version: workflow.version,
          enabled: workflow.enabled,
          inputType: workflow.inputType,
          definition: {
            'nodes': [
              {
                'id': 'run',
                'kind': 'action',
                'action': {
                  'provider': 'Console',
                  'function': 'run',
                  'timeout_seconds': 60,
                  'configuration': {},
                },
              },
            ],
          },
        ),
        null,
      );

      expect(nodes[0].data.summary, 'Console.run');
    });

    test('resolves run result metadata from workflow node action configuration', () {
      final results = workflowNodeResultMetadata(
        {
          'id': 'run',
          'kind': 'action',
          'action': {'provider': 'Console', 'function': 'run', 'configuration': {}},
          'action_name': 'Legacy',
          'action_function': 'ignored',
        },
        [
          ProviderMetadata(
            name: 'Console',
            actions: [
              ActionMetadata(
                functionName: 'run',
                parameters: const [],
                results: [
                  ActionResultMetadata(
                    name: 'stdout',
                    ty: RuninatorTypeString(),
                    label: 'Standard Output',
                  ),
                ],
              ),
            ],
            metadata: const ProviderRuntimeMetadata(credentialScopes: [], contract: null),
          ),
          ProviderMetadata(
            name: 'Legacy',
            actions: [
              ActionMetadata(
                functionName: 'ignored',
                parameters: const [],
                results: [ActionResultMetadata(name: 'legacy', ty: RuninatorTypeString())],
              ),
            ],
            metadata: const ProviderRuntimeMetadata(credentialScopes: [], contract: null),
          ),
        ],
      );

      expect(results.length, 1);
      expect(results[0].name, 'stdout');
      expect(results[0].label, 'Standard Output');
    });

    test('builds transition edges', () {
      final edges = buildGraphEdgeModels(workflow);
      expect(edges[0].source, 'a');
      expect(edges[0].target, 'b');
      expect(edges[0].label, 'next');
      expect(edges[0].type, 'workflow');
      expect(edges[0].data?.kind, WorkflowEditorEdgeKind.direct);
      expect(edges[0].data?.transitionKey, WorkflowDirectTransitionKey.next);
      expect(edges[0].data?.sourceHandle, 'source:direct.next');
      expect(edges[0].data?.targetHandle, 'target:in');
      expect(edges[0].data?.edgeStyle, WorkflowEdgeStyle.square);
      expect(edges[0].data?.editable, isTrue);
    });

    test('persists connection handle choices in edge data', () {
      final draft = _cloneWorkflow(workflow);
      setWorkflowEdgeHandles(draft.definition, 'a', 'next', sourceHandle: 'right', targetHandle: 'left');
      final edge = buildGraphEdgeModels(draft)[0];
      expect(edge.sourceHandle, 'right');
      expect(edge.targetHandle, 'left');
      expect(edge.data?.sourceHandle, 'right');
      expect(edge.data?.targetHandle, 'left');
    });

    test('persists edge style choices in edge data', () {
      final draft = _cloneWorkflow(workflow);
      setWorkflowEdgeHandles(
        draft.definition,
        'a',
        'next',
        sourceHandle: 'right',
        targetHandle: 'left',
        edgeStyle: WorkflowEdgeStyle.bezier,
      );
      var edge = buildGraphEdgeModels(draft)[0];
      expect(edge.type, 'workflow');
      expect(edge.data?.edgeStyle, WorkflowEdgeStyle.bezier);
      final edgeDraft = workflowEdgeEditorDraft(draft, edge)!;
      final updatedDraft = edgeDraft.copyWith(edgeStyle: WorkflowEdgeStyle.straight);
      expect(applyWorkflowEdgeEditorDraft(draft.definition, edge, updatedDraft).ok, isTrue);
      edge = buildGraphEdgeModels(draft)[0];
      expect(edge.data?.edgeStyle, WorkflowEdgeStyle.straight);
    });

    test('persists and clears manual edge label offsets', () {
      final draft = _cloneWorkflow(workflow);
      setWorkflowEdgeHandles(
        draft.definition,
        'a',
        'next',
        sourceHandle: 'right',
        targetHandle: 'left',
        edgeStyle: WorkflowEdgeStyle.bezier,
      );
      var edge = buildGraphEdgeModels(draft)[0];
      setWorkflowEdgeLabelOffset(draft.definition, edge, const WorkflowEdgeLabelOffset(x: 24, y: -12));
      edge = buildGraphEdgeModels(draft)[0];
      expect(edge.data?.labelOffset?.x, 24);
      expect(edge.data?.labelOffset?.y, -12);
      setWorkflowEdgeHandles(
        draft.definition,
        'a',
        'next',
        sourceHandle: 'bottom',
        targetHandle: 'top',
        edgeStyle: WorkflowEdgeStyle.square,
      );
      edge = buildGraphEdgeModels(draft)[0];
      expect(edge.data?.labelOffset?.x, 24);
      setWorkflowEdgeLabelOffset(draft.definition, edge, null);
      edge = buildGraphEdgeModels(draft)[0];
      expect(edge.data?.labelOffset, isNull);
    });

    test('persists and clears manual edge label anchors', () {
      final draft = _cloneWorkflow(workflow);
      setWorkflowEdgeHandles(
        draft.definition,
        'a',
        'next',
        sourceHandle: 'right',
        targetHandle: 'left',
        edgeStyle: WorkflowEdgeStyle.bezier,
      );
      var edge = buildGraphEdgeModels(draft)[0];
      setWorkflowEdgeLabelAnchor(draft.definition, edge, const WorkflowEdgeLabelAnchor(position: 0.25));
      edge = buildGraphEdgeModels(draft)[0];
      expect(edge.data?.labelAnchor?.position, 0.25);
      final edgeDraft = workflowEdgeEditorDraft(draft, edge)!;
      expect(edgeDraft.labelAnchor, 25);
      final updatedDraft = edgeDraft.copyWith(labelAnchor: 75);
      expect(applyWorkflowEdgeEditorDraft(draft.definition, edge, updatedDraft).ok, isTrue);
      edge = buildGraphEdgeModels(draft)[0];
      expect(edge.data?.labelAnchor?.position, 0.75);
      setWorkflowEdgeLabelAnchor(draft.definition, edge, null);
      edge = buildGraphEdgeModels(draft)[0];
      expect(edge.data?.labelAnchor, isNull);
    });

    test('generates semantic handles for rich workflow nodes', () {
      expect(
        workflowNodeSemanticHandles({
          'id': 'guard',
          'kind': 'condition',
          'transitions': {
            'branches': [
              {'target': {r'$node': 'end'}},
            ],
          },
        }).map((handle) => handle.id),
        containsAll(['target:in', 'source:branch.0', 'source:branch.new']),
      );
      expect(
        workflowNodeSemanticHandles(looseMap({
          'id': 'route',
          'kind': 'switch',
          'parameters': {'cases': [<String, Object?>{}]},
        })).map((handle) => handle.semanticOptionId),
        containsAll(['control:cases:0', 'control:default']),
      );
      expect(
        workflowNodeSemanticHandles({
          'id': 'fanout',
          'kind': 'parallel',
          'parameters': {
            'branches': [{r'$node': 'a'}],
          },
        }).map((handle) => handle.semanticOptionId),
        contains('control:branches:0'),
      );
      expect(
        workflowNodeSemanticHandles({
          'id': 'join',
          'kind': 'join',
          'parameters': {
            'wait_for': [{r'$node': 'a'}],
          },
        }).map((handle) => handle.semanticOptionId),
        contains('control:wait_for:0'),
      );
      expect(
        workflowNodeSemanticHandles({'id': 'guard', 'kind': 'try'}).map((handle) => handle.semanticOptionId),
        containsAll(['control:body', 'control:catch']),
      );
      expect(
        workflowNodeSemanticHandles({'id': 'batch', 'kind': 'map'}).map((handle) => handle.semanticOptionId),
        contains('control:target'),
      );
      expect(
        workflowNodeSemanticHandles({'id': 'task', 'kind': 'action'}).map((handle) => handle.semanticOptionId),
        contains('direct:next'),
      );
    });

    test('rejects only exact same connection point loops', () {
      expect(
        isSameConnectionPointLoop(source: 'a', target: 'a', sourceHandle: 'top', targetHandle: 'top'),
        isTrue,
      );
      expect(
        isSameConnectionPointLoop(source: 'a', target: 'a', sourceHandle: 'top', targetHandle: 'bottom'),
        isFalse,
      );
      expect(
        isSameConnectionPointLoop(source: 'a', target: 'b', sourceHandle: 'top', targetHandle: 'top'),
        isFalse,
      );
    });

    test('builds rich control-flow parameter edges', () {
      final rich = _cloneWorkflow(workflow);
      rich.definition = looseMap({
        'nodes': [
          {
            'id': 'route',
            'kind': 'switch',
            'parameters': {
              'cases': [
                {'target': {r'$node': 'fanout'}},
              ],
              'default': {r'$node': 'done'},
            },
          },
          {
            'id': 'fanout',
            'kind': 'parallel',
            'parameters': {
              'branches': [{r'$node': 'a'}, {r'$node': 'b'}],
            },
          },
          {
            'id': 'join',
            'kind': 'join',
            'parameters': {
              'wait_for': [{r'$node': 'a'}, {r'$node': 'b'}],
            },
          },
          {
            'id': 'guard',
            'kind': 'try',
            'parameters': {
              'body': {r'$node': 'body'},
              'catch': {r'$node': 'recover'},
              'finally': {r'$node': 'cleanup'},
            },
          },
          {
            'id': 'batch',
            'kind': 'map',
            'parameters': {'target': {r'$node': 'item'}},
          },
          {
            'id': 'race',
            'kind': 'race',
            'parameters': {
              'branches': [{r'$node': 'fast'}, {r'$node': 'slow'}],
            },
          },
          {'id': 'a', 'kind': 'output'},
          {'id': 'b', 'kind': 'output'},
          {'id': 'body', 'kind': 'output'},
          {'id': 'recover', 'kind': 'output'},
          {'id': 'cleanup', 'kind': 'output'},
          {'id': 'item', 'kind': 'output'},
          {'id': 'fast', 'kind': 'output'},
          {'id': 'slow', 'kind': 'output'},
          {'id': 'done', 'kind': 'end'},
        ],
      });

      final edges = buildGraphEdgeModels(rich);
      expect(edges.any((edge) => edge.source == 'route' && edge.target == 'fanout' && edge.label == 'case 1'), isTrue);
      expect(edges.any((edge) => edge.source == 'route' && edge.target == 'done' && edge.label == 'default'), isTrue);
      expect(edges.any((edge) => edge.source == 'fanout' && edge.target == 'a' && edge.label == 'branch'), isTrue);
      expect(edges.any((edge) => edge.source == 'join' && edge.target == 'b' && edge.label == 'wait_for'), isTrue);
      expect(edges.any((edge) => edge.source == 'guard' && edge.target == 'body' && edge.label == 'body'), isTrue);
      final bodyEdge = edges.firstWhere((edge) => edge.label == 'body');
      expect(bodyEdge.data?.kind, WorkflowEditorEdgeKind.control);
      expect(bodyEdge.data?.editable, isTrue);
      expect(bodyEdge.data?.parameterKey, 'body');
    });

    test('creates default nodes for editor palette kinds', () {
      final nodes = <JsonRecord>[{'id': 'approval', 'kind': 'approval'}];
      expect(createWorkflowNode('approval', nodes)['id'], 'approval_2');
      expect(createWorkflowNode('approval', nodes)['kind'], 'approval');
      expect(
        asRecord(createWorkflowNode('approval', nodes)['parameters'])['approval_type'],
        'generic',
      );
      final conditionNode = createWorkflowNode('condition', nodes);
      expect(recordArray(asRecord(conditionNode['transitions'])['branches']).length, 1);
      expect(createWorkflowNode('action', nodes)['kind'], 'action');
      expect(asRecord(createWorkflowNode('action', nodes)['action'])['provider'], '');

      for (final kind in [
        'action',
        'approval',
        'loop',
        'condition',
        'wait',
        'switch',
        'toggle',
        'percentage',
        'parallel',
        'join',
        'try',
        'map',
        'race',
        'output',
        'input',
        'subflow',
      ]) {
        expect(asRecord(createWorkflowNode(kind, nodes)['retry'])['max_attempts'], 1);
      }

      expect(asRecord(createWorkflowNode('toggle', nodes)['parameters'])['on'], {r'$node': 'end'});
      expect(asRecord(createWorkflowNode('percentage', nodes)['parameters'])['buckets'], isEmpty);
    });

    test('creates workflow trigger drafts with kind-specific defaults', () {
      const triggerWorkflowId = '00000000-0000-0000-0000-000000000042';
      final cron = newWorkflowTriggerDraft(triggerWorkflowId, WorkflowTriggerKind.cron);
      expect(cron.workflowId, triggerWorkflowId);
      expect(cron.kind, WorkflowTriggerKind.cron);
      expect(cron.enabled, isTrue);
      expect(cron.configuration['cron'], '0 * * * *');
      expect(cron.configuration['parameters'], isEmpty);

      final manual = newWorkflowTriggerDraft(triggerWorkflowId, WorkflowTriggerKind.manual);
      expect(manual.kind, WorkflowTriggerKind.manual);
      expect(manual.configuration, isEmpty);
    });

    test('generates stable unique node ids', () {
      expect(
        uniqueWorkflowNodeId([
          {'id': 'task'},
          {'id': 'task_2'},
        ], 'task'),
        'task_3',
      );
      expect(uniqueWorkflowNodeId([], 'manual approval'), 'manual_approval');
    });

    test('adds direct transitions using requested or available keys', () {
      final node = looseMap({
        'id': 'a',
        'transitions': {'next': {r'$node': 'b'}},
      });
      expect(addDirectTransition(node, 'c', 'on_failure'), 'on_failure');
      expect(node['transitions'], containsPair('on_failure', {r'$node': 'c'}));
      expect(addDirectTransition(node, 'd', 'branches'), 'on_success');
      expect(node['transitions'], containsPair('on_success', {r'$node': 'd'}));
    });

    test('offers and applies semantic edge operations for rich nodes', () {
      final condition = looseMap({'id': 'guard', 'kind': 'condition', 'transitions': {}});
      expect(
        workflowEdgeSemanticOptions(condition).map((option) => option.id),
        contains('branch:new'),
      );
      expect(applyWorkflowEdgeSemantic(condition, 'approved', 'branch:new'), 'branches.0');
      expect(
        asJsonArray(asRecord(condition['transitions'])['branches'])[0],
        {
          'when': {'value': {r'$ref': {'params': ['value']}}, 'equals': true},
          'target': {r'$node': 'approved'},
        },
      );

      final route = looseMap({
        'id': 'route',
        'kind': 'switch',
        'parameters': {'cases': <Object?>[]},
      });
      expect(
        workflowEdgeSemanticOptions(route).map((option) => option.id),
        containsAll(['control:cases:new', 'control:default']),
      );
      expect(applyWorkflowEdgeSemantic(route, 'fanout', 'control:cases:new'), 'cases.0');
      expect(applyWorkflowEdgeSemantic(route, 'done', 'control:default'), 'default');
      expect(asRecord(asJsonArray(asRecord(route['parameters'])['cases'])[0])['equals'], isTrue);
      expect(asRecord(route['parameters'])['default'], {r'$node': 'done'});
    });

    test('identifies edge semantic option ids', () {
      expect(
        workflowEdgeOptionId(
          GraphEdgeLike(
            source: 'a',
            target: 'b',
            data: const WorkflowEditorEdgeData(
              kind: WorkflowEditorEdgeKind.direct,
              transitionKey: WorkflowDirectTransitionKey.next,
              editable: true,
            ),
          ),
        ),
        'direct:next',
      );
      expect(
        workflowEdgeOptionId(
          GraphEdgeLike(
            source: 'a',
            target: 'b',
            data: const WorkflowEditorEdgeData(
              kind: WorkflowEditorEdgeKind.branch,
              branchIndex: 2,
              editable: true,
            ),
          ),
        ),
        'branch:2',
      );
      expect(
        workflowEdgeOptionId(
          GraphEdgeLike(
            source: 'a',
            target: 'b',
            data: const WorkflowEditorEdgeData(
              kind: WorkflowEditorEdgeKind.control,
              parameterKey: 'branches',
              parameterIndex: 1,
              editable: true,
            ),
          ),
        ),
        'control:branches:1',
      );
    });

    test('reads editable details from condition branch and switch case edges', () {
      final rich = _cloneWorkflow(workflow);
      rich.definition = looseMap({
        'nodes': [
          {
            'id': 'guard',
            'kind': 'condition',
            'transitions': {
              'branches': [
                {
                  'label': 'approved',
                  'when': {'value': {r'$ref': {'params': ['approved']}}, 'equals': true},
                  'target': {r'$node': 'ok'},
                },
              ],
            },
          },
          {
            'id': 'route',
            'kind': 'switch',
            'parameters': {
              'cases': [
                {'label': 'premium', 'not_equals': 'free', 'target': {r'$node': 'done'}},
              ],
            },
          },
          {'id': 'ok', 'kind': 'output'},
          {'id': 'done', 'kind': 'end'},
        ],
      });
      final edges = buildGraphEdgeModels(rich);
      final branchDraft = workflowEdgeEditorDraft(
        rich,
        edges.firstWhere((edge) => edge.source == 'guard'),
      );
      final caseDraft = workflowEdgeEditorDraft(
        rich,
        edges.firstWhere((edge) => edge.source == 'route'),
      );

      expect(branchDraft?.optionId, 'branch:0');
      expect(branchDraft?.label, 'approved');
      expect(branchDraft?.canEditCondition, isTrue);
      expect(branchDraft?.canMove, isTrue);
      expect(branchDraft?.orderIndex, 0);
      expect(branchDraft?.orderCount, 1);
      expect(jsonDecode(branchDraft!.whenJson), {
        'value': {r'$ref': {'params': ['approved']}},
        'equals': true,
      });

      expect(caseDraft?.optionId, 'control:cases:0');
      expect(caseDraft?.label, 'premium');
      expect(caseDraft?.matchKind, WorkflowEdgeEditorMatchKind.notEquals);
      expect(caseDraft?.canEditSwitchCase, isTrue);
      expect(jsonDecode(caseDraft!.matchJson), 'free');
    });

    test('applies condition branch label, predicate, and target edits', () {
      final rich = _cloneWorkflow(workflow);
      rich.definition = looseMap({
        'nodes': [
          {
            'id': 'guard',
            'kind': 'condition',
            'transitions': {
              'branches': [
                {'when': {'equals': true}, 'target': {r'$node': 'ok'}},
              ],
            },
          },
          {'id': 'ok', 'kind': 'output'},
          {'id': 'fail', 'kind': 'end'},
        ],
      });
      final edge = buildGraphEdgeModels(rich).firstWhere((item) => item.source == 'guard');
      final draft = workflowEdgeEditorDraft(rich, edge)!.copyWith(
        label: 'rejected',
        whenJson: jsonEncode({
          'value': {r'$ref': {'params': ['approved']}},
          'equals': false,
        }),
        target: 'fail',
      );

      expect(applyWorkflowEdgeEditorDraft(rich.definition, edge, draft).semanticKey, 'branches.0');
      final guardNode = (rich.definition['nodes'] as List)[0] as JsonRecord;
      final branches = asJsonArray(asRecord(guardNode['transitions'])['branches']);
      expect(branches[0], {
        'label': 'rejected',
        'when': {'value': {r'$ref': {'params': ['approved']}}, 'equals': false},
        'target': {r'$node': 'fail'},
      });
    });

    test('applies switch case match edits and default target edits', () {
      final rich = _cloneWorkflow(workflow);
      rich.definition = looseMap({
        'nodes': [
          {
            'id': 'route',
            'kind': 'switch',
            'parameters': {
              'cases': [
                {'equals': 'basic', 'target': {r'$node': 'a'}},
              ],
              'default': {r'$node': 'b'},
            },
          },
          {'id': 'a', 'kind': 'output'},
          {'id': 'b', 'kind': 'output'},
          {'id': 'c', 'kind': 'end'},
        ],
      });
      final edges = buildGraphEdgeModels(rich);
      final caseEdge = edges.firstWhere((edge) => workflowEdgeOptionId(edge) == 'control:cases:0');
      final caseDraft = workflowEdgeEditorDraft(rich, caseEdge)!.copyWith(
        label: 'not premium',
        matchKind: WorkflowEdgeEditorMatchKind.notEquals,
        matchJson: jsonEncode('premium'),
        target: 'c',
      );

      expect(applyWorkflowEdgeEditorDraft(rich.definition, caseEdge, caseDraft).semanticKey, 'cases.0');
      final routeNode = (rich.definition['nodes'] as List)[0] as JsonRecord;
      expect(asJsonArray(asRecord(routeNode['parameters'])['cases'])[0], {
        'label': 'not premium',
        'not_equals': 'premium',
        'target': {r'$node': 'c'},
      });

      final defaultEdge = buildGraphEdgeModels(rich)
          .firstWhere((edge) => workflowEdgeOptionId(edge) == 'control:default');
      final defaultDraft = workflowEdgeEditorDraft(rich, defaultEdge)!.copyWith(target: 'c');
      expect(applyWorkflowEdgeEditorDraft(rich.definition, defaultEdge, defaultDraft).semanticKey, 'default');
      expect(asRecord(routeNode['parameters'])['default'], {r'$node': 'c'});
    });

    test('moves condition branches and switch cases while preserving edge handle metadata', () {
      final rich = _cloneWorkflow(workflow);
      rich.definition = looseMap({
        'nodes': [
          {
            'id': 'guard',
            'kind': 'condition',
            'transitions': {
              'branches': [
                {'label': 'first', 'when': {'equals': true}, 'target': {r'$node': 'a'}},
                {'label': 'second', 'when': {'equals': false}, 'target': {r'$node': 'b'}},
              ],
            },
          },
          {
            'id': 'route',
            'kind': 'switch',
            'parameters': {
              'cases': [
                {'label': 'case a', 'equals': 'a', 'target': {r'$node': 'a'}},
                {'label': 'case b', 'equals': 'b', 'target': {r'$node': 'b'}},
              ],
            },
          },
          {'id': 'a', 'kind': 'output'},
          {'id': 'b', 'kind': 'end'},
        ],
        'ui': {
          'edge_handles': {
            'guard:branches.0': {'sourceHandle': 'left', 'targetHandle': 'right'},
            'guard:branches.1': {'sourceHandle': 'right', 'targetHandle': 'left'},
            'route:cases.0': {'sourceHandle': 'top', 'targetHandle': 'bottom'},
            'route:cases.1': {'sourceHandle': 'bottom', 'targetHandle': 'top'},
          },
        },
      });
      final branchEdge =
          buildGraphEdgeModels(rich).firstWhere((edge) => workflowEdgeOptionId(edge) == 'branch:0');
      final branchDraft = workflowEdgeEditorDraft(rich, branchEdge)!;
      final caseEdge = buildGraphEdgeModels(rich)
          .firstWhere((edge) => workflowEdgeOptionId(edge) == 'control:cases:1');
      final caseDraft = workflowEdgeEditorDraft(rich, caseEdge)!;

      final movedBranch = moveWorkflowEdgeEditorDraft(rich.definition, branchDraft, 1);
      expect(movedBranch.ok, isTrue);
      expect(movedBranch.draft?.optionId, 'branch:1');

      final guardNode = (rich.definition['nodes'] as List)[0] as JsonRecord;
      final guardBranches = asJsonArray(asRecord(guardNode['transitions'])['branches']);
      expect(
        guardBranches.map((branch) => asRecord(branch)['label']).toList(),
        ['second', 'first'],
      );
      final edgeHandles = asRecord(asRecord(rich.definition['ui'])['edge_handles']);
      expect(edgeHandles['guard:branches.0'], {'sourceHandle': 'right', 'targetHandle': 'left'});

      final movedCase = moveWorkflowEdgeEditorDraft(rich.definition, caseDraft, -1);
      expect(movedCase.ok, isTrue);
      expect(movedCase.draft?.optionId, 'control:cases:0');

      final routeNode = (rich.definition['nodes'] as List)[1] as JsonRecord;
      final routeCases = asJsonArray(asRecord(routeNode['parameters'])['cases']);
      expect(
        routeCases.map((switchCase) => asRecord(switchCase)['label']).toList(),
        ['case b', 'case a'],
      );
    });

    test('edits ordered parallel, race, and join target arrays', () {
      final rich = _cloneWorkflow(workflow);
      rich.definition = looseMap({
        'nodes': [
          {
            'id': 'fanout',
            'kind': 'parallel',
            'parameters': {
              'branches': [{r'$node': 'a'}, {r'$node': 'b'}],
            },
          },
          {
            'id': 'race',
            'kind': 'race',
            'parameters': {
              'branches': [{r'$node': 'a'}, {r'$node': 'b'}],
            },
          },
          {
            'id': 'join',
            'kind': 'join',
            'parameters': {
              'wait_for': [{r'$node': 'a'}, {r'$node': 'b'}],
            },
          },
          {'id': 'a', 'kind': 'output'},
          {'id': 'b', 'kind': 'output'},
          {'id': 'c', 'kind': 'end'},
        ],
      });
      final parallelEdge = buildGraphEdgeModels(rich).firstWhere(
        (edge) => edge.source == 'fanout' && workflowEdgeOptionId(edge) == 'control:branches:0',
      );
      final raceEdge = buildGraphEdgeModels(rich).firstWhere(
        (edge) => edge.source == 'race' && workflowEdgeOptionId(edge) == 'control:branches:1',
      );
      final joinEdge = buildGraphEdgeModels(rich).firstWhere(
        (edge) => edge.source == 'join' && workflowEdgeOptionId(edge) == 'control:wait_for:0',
      );

      final parallelDraft = workflowEdgeEditorDraft(rich, parallelEdge)!.copyWith(target: 'c');
      expect(applyWorkflowEdgeEditorDraft(rich.definition, parallelEdge, parallelDraft).semanticKey, 'branches.0');

      final raceDraft = workflowEdgeEditorDraft(rich, raceEdge)!.copyWith(target: 'c');
      expect(applyWorkflowEdgeEditorDraft(rich.definition, raceEdge, raceDraft).semanticKey, 'branches.1');

      final joinDraft = workflowEdgeEditorDraft(rich, joinEdge)!.copyWith(target: 'c');
      expect(applyWorkflowEdgeEditorDraft(rich.definition, joinEdge, joinDraft).semanticKey, 'wait_for.0');

      final fanoutNode = (rich.definition['nodes'] as List)[0] as JsonRecord;
      final raceNode = (rich.definition['nodes'] as List)[1] as JsonRecord;
      final joinNode = (rich.definition['nodes'] as List)[2] as JsonRecord;
      expect(asJsonArray(asRecord(fanoutNode['parameters'])['branches']), [
        {r'$node': 'c'},
        {r'$node': 'b'},
      ]);
      expect(asJsonArray(asRecord(raceNode['parameters'])['branches']), [
        {r'$node': 'a'},
        {r'$node': 'c'},
      ]);
      expect(asJsonArray(asRecord(joinNode['parameters'])['wait_for']), [
        {r'$node': 'c'},
        {r'$node': 'b'},
      ]);
    });

    test('rejects invalid predicate JSON without mutating the workflow', () {
      final rich = _cloneWorkflow(workflow);
      rich.definition = looseMap({
        'nodes': [
          {
            'id': 'guard',
            'kind': 'condition',
            'transitions': {
              'branches': [
                {'when': {'equals': true}, 'target': {r'$node': 'ok'}},
              ],
            },
          },
          {'id': 'ok', 'kind': 'output'},
          {'id': 'fail', 'kind': 'end'},
        ],
      });
      final before = jsonEncode(rich.definition);
      final edge = buildGraphEdgeModels(rich).firstWhere((item) => item.source == 'guard');
      final draft = workflowEdgeEditorDraft(rich, edge)!.copyWith(whenJson: '{', target: 'fail');

      final result = applyWorkflowEdgeEditorDraft(rich.definition, edge, draft);
      expect(result.ok, isFalse);
      expect(result.message, 'Condition branch predicate must be valid JSON');
      expect(jsonEncode(rich.definition), before);
    });

    test('maps workflow validation issues to nodes and edges', () {
      final definition = looseMap({
        'start': 'missing_start',
        'nodes': [
          {
            'id': 'task',
            'kind': 'action',
            'action': {'provider': 'Unknown', 'function': 'run', 'configuration': {}},
            'transitions': {'next': {r'$node': 'missing'}},
          },
          {
            'id': 'task',
            'kind': 'output',
            'parameters': {'data': {r'$ref': {'node': 'missing'}}},
          },
          {
            'id': 'guard',
            'kind': 'condition',
            'transitions': {
              'branches': [
                {'when': {'value': '{{legacy}}'}},
              ],
            },
          },
          {
            'id': 'route',
            'kind': 'switch',
            'parameters': {
              'cases': [
                {'equals': true},
              ],
            },
          },
        ],
      });

      final issues = validateWorkflowIssues(definition, [
        const ProviderMetadata(
          name: 'Console',
          actions: [],
          metadata: ProviderRuntimeMetadata(credentialScopes: [], contract: null),
        ),
      ]);

      expect(
        issues.any(
          (issue) =>
              issue.nodeId == 'missing_start' &&
              issue.message == 'Workflow start references missing node missing_start',
        ),
        isTrue,
      );
      expect(issues.any((issue) => issue.nodeId == 'task' && issue.message == 'Duplicate node ID task'), isTrue);
      expect(
        issues.any(
          (issue) =>
              issue.nodeId == 'task' &&
              issue.edgeKey == 'task:next' &&
              issue.message == 'task.next references missing node missing',
        ),
        isTrue,
      );
      expect(
        issues.any((issue) => issue.nodeId == 'task' && issue.message == 'task references unknown provider Unknown'),
        isTrue,
      );
    });

    test('treats whitespace-only required action inputs as missing', () {
      final providers = [
        ProviderMetadata(
          name: 'Console',
          actions: [
            ActionMetadata(
              functionName: 'run',
              parameters: [
                ActionParameterMetadata(
                  name: 'command',
                  required: true,
                  secret: false,
                  ty: RuninatorTypeString(),
                ),
              ],
              results: const [],
            ),
          ],
          metadata: const ProviderRuntimeMetadata(credentialScopes: [], contract: null),
        ),
      ];
      final blank = looseMap({
        'start': 'task',
        'nodes': [
          {
            'id': 'task',
            'kind': 'action',
            'action': {'provider': 'Console', 'function': 'run', 'configuration': {}},
            'parameters': {'command': '   '},
          },
        ],
      });
      expect(
        validateWorkflowIssues(blank, providers).any((issue) => issue.message == 'task: command is required'),
        isTrue,
      );

      final provided = looseMap({
        'start': 'task',
        'nodes': [
          {
            'id': 'task',
            'kind': 'action',
            'action': {'provider': 'Console', 'function': 'run', 'configuration': {}},
            'parameters': {'command': 'echo hi'},
          },
        ],
      });
      expect(
        validateWorkflowIssues(provided, providers).any((issue) => issue.message.contains('command is required')),
        isFalse,
      );
    });

    test('applies inline node edits while preserving layout and references', () {
      final definition = looseMap({
        'start': 'start',
        'nodes': [
          {'id': 'start', 'kind': 'start', 'transitions': {'next': {r'$node': 'approve'}}},
          {
            'id': 'approve',
            'kind': 'approval',
            'parameters': {'prompt': 'Old prompt'},
            'transitions': {'next': {r'$node': 'end'}},
          },
          {'id': 'end', 'kind': 'end'},
        ],
        'ui': {
          'layout': {
            'nodes': {'approve': {'x': 20, 'y': 40}},
          },
          'edge_handles': {
            'approve:next': {
              'sourceHandle': 'right',
              'targetHandle': 'left',
              'labelAnchor': {'position': 0.25},
            },
          },
        },
      });

      final result = applyWorkflowInlineNodeEdit(definition, 'approve', 'review', 'Review Step');
      expect(result.ok, isTrue);
      expect(result.nodeId, 'review');

      final ui = asRecord(definition['ui']);
      final layoutNodes = asRecord(asRecord(ui['layout'])['nodes']);
      layoutNodes['review'] = layoutNodes.remove('approve');

      final nodes = definition['nodes'] as List;
      expect((nodes[1] as JsonRecord)['id'], 'review');
      expect((nodes[1] as JsonRecord)['name'], 'Review Step');
      expect(asRecord((nodes[0] as JsonRecord)['transitions'])['next'], {r'$node': 'review'});
      expect(layoutNodes['review'], {'x': 20, 'y': 40});
      final edgeHandles = asRecord(ui['edge_handles']);
      expect(edgeHandles['review:next'], {
        'sourceHandle': 'right',
        'targetHandle': 'left',
        'labelAnchor': {'position': 0.25},
      });
      expect(edgeHandles.containsKey('approve:next'), isFalse);
    });

    test('edits condition branches', () {
      final node = looseMap({'id': 'guard', 'kind': 'condition', 'transitions': {}});
      setConditionBranch(node, 0, {'equals': true}, 'ok');
      setConditionBranch(node, 1, {'equals': false}, 'fail');
      expect(asJsonArray(asRecord(node['transitions'])['branches']), [
        {'when': {'equals': true}, 'target': {r'$node': 'ok'}},
        {'when': {'equals': false}, 'target': {r'$node': 'fail'}},
      ]);
      removeConditionBranch(node, 0);
      expect(asJsonArray(asRecord(node['transitions'])['branches']), [
        {'when': {'equals': false}, 'target': {r'$node': 'fail'}},
      ]);
    });

    test('removes editable graph edges without touching control-flow edges', () {
      final node = looseMap({
        'id': 'a',
        'transitions': {
          'next': {r'$node': 'b'},
          'branches': [
            {'when': {}, 'target': {r'$node': 'c'}},
          ],
        },
      });
      expect(
        removeEditableEdge(
          node,
          GraphEdgeLike(
            source: 'a',
            target: 'b',
            data: const WorkflowEditorEdgeData(
              kind: WorkflowEditorEdgeKind.direct,
              transitionKey: WorkflowDirectTransitionKey.next,
              editable: true,
            ),
          ),
        ),
        isTrue,
      );
      expect(asRecord(node['transitions']).containsKey('next'), isFalse);
      expect(
        removeEditableEdge(
          node,
          GraphEdgeLike(
            source: 'a',
            target: 'c',
            data: const WorkflowEditorEdgeData(
              kind: WorkflowEditorEdgeKind.branch,
              branchIndex: 0,
              editable: true,
            ),
          ),
        ),
        isTrue,
      );
      expect(asJsonArray(asRecord(node['transitions'])['branches']), isEmpty);

      final controlNode = looseMap({'id': 'route', 'parameters': {'target': {r'$node': 'item'}}});
      expect(
        removeEditableEdge(
          controlNode,
          GraphEdgeLike(
            source: 'route',
            target: 'item',
            data: const WorkflowEditorEdgeData(kind: WorkflowEditorEdgeKind.control, editable: false),
          ),
        ),
        isFalse,
      );
      expect(asRecord(controlNode['parameters'])['target'], {r'$node': 'item'});
    });

    test('normalizes legacy definitions with required start and end nodes', () {
      final normalized = normalizeWorkflowDefinition(workflow);
      expect(normalized.definition['start'], 'start');
      final kinds = (normalized.definition['nodes'] as List).map((node) => (node as JsonRecord)['kind']).toList();
      expect(kinds, ['start', 'action', 'action', 'end', 'fail']);
      final startNode =
          (normalized.definition['nodes'] as List).firstWhere((node) => (node as JsonRecord)['id'] == 'start')
              as JsonRecord;
      expect(asRecord(startNode['transitions']), isEmpty);
      final layout = asRecord(asRecord(asRecord(normalized.definition['ui'])['layout'])['nodes']);
      expect(layout['a'], {'x': 10, 'y': 20});
    });

    test('uses legacy layout positions', () {
      final legacy = _cloneWorkflow(workflow);
      legacy.definition = looseMap({
        ...legacy.definition,
        'ui': {
          'layout': {'a': {'x': 30, 'y': 40}},
        },
      });
      expect(buildGraphNodeModels(legacy, null)[0].position.x, 30);
      expect(buildGraphNodeModels(legacy, null)[0].position.y, 40);
    });

    test('auto arranges direct workflow nodes from start to end', () {
      final arranged = autoArrangeWorkflowLayout({
        'start': 'start',
        'nodes': [
          {'id': 'start', 'kind': 'start', 'transitions': {'next': {r'$node': 'task'}}},
          {'id': 'task', 'kind': 'action', 'transitions': {'next': {r'$node': 'end'}}},
          {'id': 'end', 'kind': 'end'},
        ],
      });
      expect(arranged['start']!.x, lessThan(arranged['task']!.x));
      expect(arranged['task']!.x, lessThan(arranged['end']!.x));
      expect(arranged['start']!.y, arranged['task']!.y);
      expect(arranged['task']!.y, arranged['end']!.y);
    });

    test('auto arranges branches on the same rank before their join', () {
      final arranged = autoArrangeWorkflowLayout({
        'start': 'start',
        'nodes': [
          {'id': 'start', 'kind': 'start', 'transitions': {'next': {r'$node': 'fanout'}}},
          {
            'id': 'fanout',
            'kind': 'parallel',
            'parameters': {
              'branches': [{r'$node': 'a'}, {r'$node': 'b'}],
            },
          },
          {'id': 'a', 'kind': 'action', 'transitions': {'next': {r'$node': 'join'}}},
          {'id': 'b', 'kind': 'action', 'transitions': {'next': {r'$node': 'join'}}},
          {
            'id': 'join',
            'kind': 'join',
            'parameters': {
              'wait_for': [{r'$node': 'a'}, {r'$node': 'b'}],
            },
            'transitions': {'next': {r'$node': 'end'}},
          },
          {'id': 'end', 'kind': 'end'},
        ],
      });

      expect(arranged['a']!.x, arranged['b']!.x);
      expect(arranged['a']!.y, isNot(arranged['b']!.y));
      expect(arranged['join']!.x, greaterThan(arranged['a']!.x));
      expect(arranged['end']!.x, greaterThan(arranged['join']!.x));
    });

    test('auto arranges cyclic nodes without recursive rank growth', () {
      final arranged = autoArrangeWorkflowLayout({
        'start': 'start',
        'nodes': [
          {'id': 'start', 'kind': 'start', 'transitions': {'next': {r'$node': 'a'}}},
          {'id': 'a', 'kind': 'action', 'transitions': {'next': {r'$node': 'b'}}},
          {
            'id': 'b',
            'kind': 'action',
            'transitions': {'next': {r'$node': 'a'}, 'on_success': {r'$node': 'end'}},
          },
          {'id': 'end', 'kind': 'end'},
        ],
      });

      expect(arranged['a']!.x, arranged['b']!.x);
      expect(arranged['a']!.y, isNot(arranged['b']!.y));
      expect(arranged['end']!.x, greaterThan(arranged['a']!.x));
    });

    test('marks the active completed end node as succeeded', () {
      final normalized = normalizeWorkflowDefinition(workflow);
      final nodes = buildGraphNodeModels(
        normalized,
        WorkflowRunDetail(
          run: WorkflowRunDetailRun(
            id: runId,
            workflowId: workflowId,
            status: 'succeeded',
            activeNodeId: 'end',
            createdAt: '',
            startedAt: null,
            finishedAt: '',
          ),
          nodes: const [],
        ),
      );
      expect(nodes.firstWhere((node) => node.id == 'end').className, 'node-success');
    });

    test('renders output nodes with the output kind instead of fail', () {
      final nodes = buildGraphNodeModels(
        WorkflowDefinition(
          id: workflowId,
          name: 'output check',
          version: '1.0.0',
          enabled: true,
          inputType: {'type': 'struct', 'fields': {}},
          definition: {
            'start': 'start',
            'nodes': [
              {'id': 'start', 'kind': 'start', 'transitions': {}},
              {
                'id': 'output_1',
                'kind': 'output',
                'parameters': {'event_type': 'workflow.output', 'data': {}},
                'transitions': {},
              },
              {'id': 'end', 'kind': 'end'},
              {'id': 'fail', 'kind': 'fail'},
            ],
          },
        ),
        null,
      );

      expect(nodes.firstWhere((node) => node.id == 'output_1').data.kind, 'output');
      expect(nodes.firstWhere((node) => node.id == 'fail').data.kind, 'fail');
    });

    test('marks the active workflow node as running before its node run appears', () {
      final nodes = buildGraphNodeModels(
        workflow,
        WorkflowRunDetail(
          run: WorkflowRunDetailRun(
            id: runId,
            workflowId: workflowId,
            status: 'running',
            activeNodeId: 'b',
            createdAt: '',
            startedAt: null,
            finishedAt: null,
          ),
          nodes: [
            WorkflowNodeRun(
              id: nodeRunId,
              workflowRunId: runId,
              nodeId: 'a',
              status: 'succeeded',
              attempt: 1,
              parameters: {},
              message: null,
            ),
          ],
        ),
      );
      final active = nodes.firstWhere((node) => node.id == 'b');
      expect(active.data.status, 'running');
      expect(active.data.running, isTrue);
      expect(active.className, 'node-running');
    });

    test('marks the active workflow node as debug paused', () {
      final nodes = buildGraphNodeModels(
        workflow,
        WorkflowRunDetail(
          run: WorkflowRunDetailRun(
            id: runId,
            workflowId: workflowId,
            status: 'debug_paused',
            activeNodeId: 'b',
            createdAt: '',
            startedAt: null,
            finishedAt: null,
          ),
          nodes: const [],
        ),
      );

      expect(nodes.firstWhere((node) => node.id == 'b').data.status, 'debug_paused');
      expect(nodes.firstWhere((node) => node.id == 'b').className, 'node-warning');
    });

    test('renders waiting workflow nodes with the waiting state', () {
      final nodes = buildGraphNodeModels(
        workflow,
        WorkflowRunDetail(
          run: WorkflowRunDetailRun(
            id: runId,
            workflowId: workflowId,
            status: 'waiting',
            activeNodeId: 'b',
            createdAt: '',
            startedAt: null,
            finishedAt: null,
          ),
          nodes: [
            WorkflowNodeRun(
              id: nodeRunId,
              workflowRunId: runId,
              nodeId: 'b',
              status: 'waiting',
              attempt: 1,
              parameters: {},
              message: null,
            ),
          ],
        ),
      );

      expect(nodes.firstWhere((node) => node.id == 'b').data.status, 'waiting');
      expect(nodes.firstWhere((node) => node.id == 'b').className, 'node-waiting');
    });

    test('builds workflow run search text with workflow identity', () {
      final text = workflowRunSearchText(
        RunSummary(
          id: searchRunId,
          workflowId: searchWorkflowId,
          status: 'failed',
          createdAt: '',
          startedAt: null,
          finishedAt: null,
        ),
        'Nightly Import',
      );
      expect(text, contains('nightly import'));
      expect(text, contains(searchWorkflowId));
    });

    test('marks the active terminal workflow node from the run status', () {
      final nodes = buildGraphNodeModels(
        workflow,
        WorkflowRunDetail(
          run: WorkflowRunDetailRun(
            id: runId,
            workflowId: workflowId,
            status: 'failed',
            activeNodeId: 'b',
            createdAt: '',
            startedAt: null,
            finishedAt: '',
          ),
          nodes: const [],
        ),
      );
      expect(nodes.firstWhere((node) => node.id == 'b').className, 'node-danger');
    });
  });
}
