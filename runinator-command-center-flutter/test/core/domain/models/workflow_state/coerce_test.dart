// unit tests for the workflow-state "frame bag" coercion pattern, mirroring
// core/domain/models/workflow-state/__tests__ intent from the ts source: every
// coerce factory must be defensive against missing keys, wrong-typed values,
// and empty/absent input.

import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/compensation_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/control_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/debug_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/loop_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/map_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/parallel_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/race_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/try_frame.dart';
import 'package:runinator_command_center_flutter/core/domain/models/workflow_state/workflow_run_state.dart';
import 'package:test/test.dart';

void main() {
  group('ControlFrame.fromCoercedJson', () {
    test('returns null for null input', () {
      expect(ControlFrame.fromCoercedJson(null), isNull);
    });

    test('returns null for an empty record', () {
      expect(ControlFrame.fromCoercedJson(<String, Object?>{}), isNull);
    });

    test('drops a wrong-typed field instead of throwing', () {
      final frame = ControlFrame.fromCoercedJson({'pause_requested': 'not-a-bool'});
      expect(frame, isNotNull);
      expect(frame!.pauseRequested, isNull);
    });

    test('keeps a well-typed field', () {
      final frame = ControlFrame.fromCoercedJson({'pause_requested': true});
      expect(frame!.pauseRequested, isTrue);
    });
  });

  group('LoopFrame.fromCoercedJson', () {
    test('returns null for an empty record (no required fields)', () {
      expect(LoopFrame.fromCoercedJson(<String, Object?>{}), isNull);
    });

    test('coerces a partial frame', () {
      final frame = LoopFrame.fromCoercedJson({'index': 3});
      expect(frame!.index, 3);
      expect(frame.returnTo, isNull);
    });
  });

  group('ParallelFrame.fromCoercedJson (required node_id)', () {
    test('returns null when node_id is missing', () {
      expect(ParallelFrame.fromCoercedJson({'remaining': <String>[]}), isNull);
    });

    test('returns null when node_id is wrong-typed', () {
      expect(ParallelFrame.fromCoercedJson({'node_id': 42}), isNull);
    });

    test('coerces when node_id is present', () {
      final frame = ParallelFrame.fromCoercedJson({'node_id': 'n1', 'remaining': ['a', 'b']});
      expect(frame!.nodeId, 'n1');
      expect(frame.remaining, ['a', 'b']);
    });
  });

  group('RaceFrame.fromCoercedJson (required node_id)', () {
    test('returns null when node_id is missing', () {
      expect(RaceFrame.fromCoercedJson(<String, Object?>{}), isNull);
    });
  });

  group('TryFrame.fromCoercedJson (required node_id + phase)', () {
    test('returns null when phase is missing', () {
      expect(TryFrame.fromCoercedJson({'node_id': 'n1'}), isNull);
    });

    test('coerces when both required fields are present', () {
      final frame = TryFrame.fromCoercedJson({'node_id': 'n1', 'phase': 'body'});
      expect(frame!.nodeId, 'n1');
      expect(frame.phase, 'body');
      expect(frame.pendingStatus, isNull);
    });
  });

  group('CompensationFrame.fromCoercedJson', () {
    test('returns null for an empty record', () {
      expect(CompensationFrame.fromCoercedJson(<String, Object?>{}), isNull);
    });

    test('coerces active_run_id', () {
      final frame = CompensationFrame.fromCoercedJson({'active_run_id': 'r1'});
      expect(frame!.activeRunId, 'r1');
    });
  });

  group('MapFrame.fromCoercedJson (required node_id + target)', () {
    test('returns null when target is missing', () {
      expect(MapFrame.fromCoercedJson({'node_id': 'n1'}), isNull);
    });

    test('does not populate in_flight/results (mirrors the ts source verbatim)', () {
      final frame = MapFrame.fromCoercedJson({
        'node_id': 'n1',
        'target': 'end',
        'in_flight': [
          {'index': 0, 'child_run_id': 'c1'}
        ],
        'results': [1, 2, 3],
      });
      expect(frame!.inFlight, isNull);
      expect(frame.results, isNull);
    });
  });

  group('DebugFrame.fromCoercedJson', () {
    test('returns null for null input', () {
      expect(DebugFrame.fromCoercedJson(null), isNull);
    });

    test('coerces breakpoints, dropping non-string entries', () {
      final frame = DebugFrame.fromCoercedJson({
        'breakpoints': ['a', 1, 'b', null],
      });
      expect(frame!.breakpoints, ['a', 'b']);
    });

    test('rejects an unrecognized debug mode', () {
      final frame = DebugFrame.fromCoercedJson({'mode': 'not_a_mode'});
      expect(frame!.mode, isNull);
    });

    test('accepts a valid debug mode', () {
      final frame = DebugFrame.fromCoercedJson({'mode': 'breakpoints'});
      expect(frame!.mode!.wire, 'breakpoints');
    });
  });

  group('WorkflowRunState.coerce', () {
    test('returns null for non-map input', () {
      expect(WorkflowRunState.coerce('not a map'), isNull);
      expect(WorkflowRunState.coerce(42), isNull);
      expect(WorkflowRunState.coerce(null), isNull);
    });

    test('a run can hold multiple frames simultaneously (not a discriminated union)', () {
      final state = WorkflowRunState.coerce({
        'loop': {'index': 1},
        'debug': {'enabled': true},
      });
      expect(state, isNotNull);
      expect(state!.loop, isNotNull);
      expect(state.debug, isNotNull);
      expect(state.debug!.enabled, isTrue);
    });

    test('watch_fired defaults to false and only true is truthy', () {
      expect(WorkflowRunState.coerce(<String, Object?>{})!.watchFired, isFalse);
      expect(WorkflowRunState.coerce({'watch_fired': 'yes'})!.watchFired, isFalse);
      expect(WorkflowRunState.coerce({'watch_fired': true})!.watchFired, isTrue);
    });
  });
}
