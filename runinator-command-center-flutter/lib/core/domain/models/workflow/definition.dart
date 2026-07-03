// port of core/domain/models/workflow/definition.ts.

import '../../json.dart';
import '../provider/runinator_type.dart';

// mutable (not final fields): the ts source's WorkflowDefinition is a plain,
// always-mutable object, and core/services/workflows/editor.ts relies on
// reassigning `workflowDraft.name`/`.version`/`.inputType`/`.definition` in
// place (e.g. after a WDL recompile). ported faithfully rather than forcing
// immutability that the source's own editing logic doesn't have.
class WorkflowDefinition {
  WorkflowDefinition({
    required this.id,
    required this.name,
    required this.version,
    required this.enabled,
    required this.inputType,
    required this.definition,
    this.orgId,
  });

  factory WorkflowDefinition.fromJson(Map<String, Object?> json) => WorkflowDefinition(
        id: json['id'] as String?,
        name: json['name'] as String,
        // semantic version string, e.g. "1.2.0".
        version: json['version'] as String,
        enabled: json['enabled'] as bool,
        inputType: asJsonObject(json['input_type']),
        definition: asJsonObject(json['definition']),
        // owning organization (tenant); null means platform-global / unassigned.
        orgId: json['org_id'] as String?,
      );

  String? id;
  String name;
  String version;
  bool enabled;
  JsonRecord inputType;
  JsonRecord definition;
  String? orgId;

  Map<String, Object?> toJson() => {
        'id': id,
        'name': name,
        'version': version,
        'enabled': enabled,
        'input_type': inputType,
        'definition': definition,
        'org_id': orgId,
      };
}

/// read the workflow input schema as a RuninatorType when present and well-formed.
RuninatorType? workflowInputType(WorkflowDefinition workflow) =>
    RuninatorType.tryParse(workflow.inputType);
