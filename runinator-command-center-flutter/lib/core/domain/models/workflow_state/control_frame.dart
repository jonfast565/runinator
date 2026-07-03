// port of core/domain/models/workflow-state/control-frame.ts.

import 'coerce_helpers.dart';

/// `state.control` bookkeeping.
class ControlFrame {
  const ControlFrame({this.pauseRequested});

  /// mirrors coerceControlFrame: returns null for a missing/empty record.
  static ControlFrame? fromCoercedJson(Object? value) {
    if (value == null) {
      return null;
    }

    final record = coerceRecord(value);
    if (record.isEmpty) {
      return null;
    }

    return ControlFrame(
      pauseRequested: record['pause_requested'] is bool ? record['pause_requested'] as bool : null,
    );
  }

  final bool? pauseRequested;

  Map<String, Object?> toJson() => {'pause_requested': pauseRequested};
}
