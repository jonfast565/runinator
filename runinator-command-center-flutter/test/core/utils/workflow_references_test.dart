import 'package:runinator_command_center_flutter/core/domain/models/index.dart';
import 'package:runinator_command_center_flutter/core/domain/models/provider/action_metadata.dart';
import 'package:runinator_command_center_flutter/core/domain/models/provider/provider_metadata.dart';
import 'package:runinator_command_center_flutter/core/domain/models/provider/runinator_type.dart';
import 'package:runinator_command_center_flutter/core/utils/workflow_references.dart';
import 'package:test/test.dart';

const inputType = RuninatorTypeStruct(
  fields: {
    'cart': RuninatorField(
      required: true,
      ty: RuninatorTypeStruct(
        fields: {
          'total': RuninatorField(required: true, ty: RuninatorTypeNumber()),
        },
      ),
    ),
    'name': RuninatorField(required: true, ty: RuninatorTypeString()),
  },
);

final providers = [
  ProviderMetadata(
    name: 'jira',
    actions: [
      ActionMetadata(
        functionName: 'search',
        parameters: const [],
        results: [
          ActionResultMetadata(
            name: 'issues',
            ty: RuninatorTypeArray(
              RuninatorTypeStruct(
                fields: {
                  'key': RuninatorField(required: true, ty: RuninatorTypeString()),
                  'fields': RuninatorField(
                    required: true,
                    ty: RuninatorTypeStruct(
                      fields: {
                        'summary': RuninatorField(required: true, ty: RuninatorTypeString()),
                      },
                    ),
                  ),
                },
              ),
            ),
          ),
          ActionResultMetadata(name: 'total', ty: RuninatorTypeInteger()),
        ],
      ),
    ],
    metadata: const ProviderRuntimeMetadata(credentialScopes: []),
  ),
];

final nodes = <Map<String, Object?>>[
  {'id': 'make_ticket', 'kind': 'action', 'action': {'provider': 'jira', 'function': 'search'}},
  {'id': 'current', 'kind': 'action', 'action': {'provider': 'jira', 'function': 'search'}},
];

void main() {
  group('workflowReferenceGroups', () {
    final groups = workflowReferenceGroups(
      WorkflowExpressionEditorContext(
        workflowInputType: inputType,
        nodes: nodes,
        currentNodeId: 'current',
        providers: providers,
      ),
    );

    test('flattens workflow parameter fields by dotted path with types', () {
      final params = groups.firstWhere((group) => group.title == 'Workflow parameters');
      final inserts = params.references.map((reference) => reference.insert).toList();
      expect(inserts, containsAll(['params.cart', 'params.cart.total', 'params.name']));
      expect(
        params.references.firstWhere((r) => r.insert == 'params.cart.total').type,
        'number',
      );
    });

    test('groups prior node outputs and excludes the current node', () {
      final references =
          groups.firstWhere((group) => group.title == 'Output of make_ticket').references;
      expect(references.map((reference) => reference.insert).toList(), [
        'make_ticket.issues',
        'make_ticket.issues.0',
        'make_ticket.issues.0.key',
        'make_ticket.issues.0.fields',
        'make_ticket.issues.0.fields.summary',
        'make_ticket.total',
      ]);
      expect(groups.any((group) => group.title == 'Output of current'), isFalse);
    });

    test('always offers the run-state roots', () {
      final roots = groups.firstWhere((group) => group.title == 'Run state');
      expect(roots.references.map((reference) => reference.insert).toList(), [
        'prev',
        'run',
        'config',
        'secret',
      ]);
    });
  });

  group('buildSampleContext', () {
    final detail = WorkflowRunDetail(
      run: WorkflowRunDetailRun(
        id: 'r1',
        workflowId: 'w1',
        status: 'succeeded',
        parameters: {'x': 1},
        createdAt: '',
        startedAt: null,
        finishedAt: null,
      ),
      nodes: [
        WorkflowNodeRun(
          id: '1',
          workflowRunId: 'r1',
          nodeId: 'a',
          status: 'succeeded',
          attempt: 1,
          parameters: {},
          outputJson: {'k': 'v'},
          message: null,
        ),
        WorkflowNodeRun(
          id: '2',
          workflowRunId: 'r1',
          nodeId: 'b',
          status: 'succeeded',
          attempt: 1,
          parameters: {},
          outputJson: {'n': 2},
          message: null,
        ),
      ],
    );

    test('mirrors the reducer context with params/steps/prev/workflow', () {
      expect(buildSampleContext(detail), {
        'params': {'x': 1},
        'steps': {
          'a': {'output': {'k': 'v'}},
          'b': {'output': {'n': 2}},
        },
        'prev': {'n': 2},
        'workflow': {'run_id': 'r1', 'workflow_id': 'w1', 'state': 'succeeded'},
      });
    });

    test('returns null without a run', () {
      expect(buildSampleContext(null), isNull);
    });
  });
}
