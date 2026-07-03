// port of core/domain/models/workflow-state/workflow-run-state.ts.
//
// typed view of `workflow_run.state`. mirrors runinator-models::WorkflowRunState;
// unmodeled keys may still exist on the wire object beside these frames.
//
// this is NOT a discriminated union (verified against the source's coerce.ts): each
// frame field is independently optional, and a run can hold several frames at once
// (e.g. simultaneously in a loop and being single-stepped in the debugger).

import '../../json.dart';
import 'coerce_helpers.dart';
import 'compensation_frame.dart';
import 'control_frame.dart';
import 'debug_frame.dart';
import 'loop_frame.dart';
import 'map_frame.dart';
import 'parallel_frame.dart';
import 'race_frame.dart';
import 'try_frame.dart';

class WorkflowRunState {
  const WorkflowRunState({
    this.control,
    this.debug,
    this.loop,
    this.parallel,
    this.map,
    this.race,
    this.try_,
    this.compensation,
    this.runMetadata,
    this.watchFired = false,
  });

  /// parse a run `state` blob into typed frames; returns null when value is not an object.
  /// mirrors coerceWorkflowRunState.
  static WorkflowRunState? coerce(Object? value) {
    if (!isJsonRecord(value)) {
      return null;
    }

    final record = value as Map<String, Object?>;

    return WorkflowRunState(
      control: ControlFrame.fromCoercedJson(record['control']),
      debug: DebugFrame.fromCoercedJson(record['debug']),
      loop: LoopFrame.fromCoercedJson(record['loop']),
      parallel: ParallelFrame.fromCoercedJson(record['parallel']),
      map: MapFrame.fromCoercedJson(record['map']),
      race: RaceFrame.fromCoercedJson(record['race']),
      try_: TryFrame.fromCoercedJson(record['try']),
      compensation: CompensationFrame.fromCoercedJson(record['compensation']),
      runMetadata: optionalJsonValue(record['run_metadata']),
      watchFired: record['watch_fired'] == true,
    );
  }

  final ControlFrame? control;
  final DebugFrame? debug;
  final LoopFrame? loop;
  final ParallelFrame? parallel;
  final MapFrame? map;
  final RaceFrame? race;
  final TryFrame? try_;
  final CompensationFrame? compensation;
  final JsonValue runMetadata;
  final bool watchFired;

  Map<String, Object?> toJson() => {
        'control': control?.toJson(),
        'debug': debug?.toJson(),
        'loop': loop?.toJson(),
        'parallel': parallel?.toJson(),
        'map': map?.toJson(),
        'race': race?.toJson(),
        'try': try_?.toJson(),
        'compensation': compensation?.toJson(),
        'run_metadata': runMetadata,
        'watch_fired': watchFired,
      };
}
