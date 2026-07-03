// port of the shared helper functions at the top of core/domain/models/workflow-state/coerce.ts.
// used by every frame's fromCoercedJson factory in this directory.

import '../../json.dart';
import '../workflow/node_kind.dart';
import 'debug_mode.dart';

List<String>? stringArrayOrNull(Object? value) {
  if (value is! List) {
    return null;
  }

  return value.whereType<String>().toList();
}

DebugMode? debugModeOrNull(Object? value) => DebugMode.fromWireOrNull(value as String?);

JsonValue optionalJsonValue(Object? value) => value == null ? null : asJsonValue(value);

JsonRecord coerceRecord(Object? value) => value == null ? <String, Object?>{} : asJsonRecord(value);

WorkflowNodeKind? workflowNodeKindOrNull(Object? value) =>
    value is String ? WorkflowNodeKind.fromWire(value) : null;
