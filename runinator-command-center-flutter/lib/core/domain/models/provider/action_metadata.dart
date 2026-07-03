// port of core/domain/models/provider/action-metadata.ts.

import '../../json.dart';
import 'runinator_type.dart';

class ActionParameterMetadata {
  const ActionParameterMetadata({
    required this.name,
    required this.ty,
    this.label,
    this.description,
    required this.required,
    this.defaultValue,
    required this.secret,
  });

  factory ActionParameterMetadata.fromJson(Map<String, Object?> json) => ActionParameterMetadata(
        name: json['name'] as String,
        ty: RuninatorType.fromJson(json['ty'] as Map<String, Object?>),
        label: json['label'] as String?,
        description: json['description'] as String?,
        required: json['required'] as bool,
        defaultValue: json.containsKey('default_value') ? asJsonValue(json['default_value']) : null,
        secret: json['secret'] as bool,
      );

  final String name;
  final RuninatorType ty;
  final String? label;
  final String? description;
  final bool required;
  final JsonValue defaultValue;
  final bool secret;

  Map<String, Object?> toJson() => {
        'name': name,
        'ty': ty.toJson(),
        'label': label,
        'description': description,
        'required': required,
        'default_value': defaultValue,
        'secret': secret,
      };
}

class ActionResultMetadata {
  const ActionResultMetadata({
    required this.name,
    required this.ty,
    this.label,
    this.description,
  });

  factory ActionResultMetadata.fromJson(Map<String, Object?> json) => ActionResultMetadata(
        name: json['name'] as String,
        ty: RuninatorType.fromJson(json['ty'] as Map<String, Object?>),
        label: json['label'] as String?,
        description: json['description'] as String?,
      );

  final String name;
  final RuninatorType ty;
  final String? label;
  final String? description;

  Map<String, Object?> toJson() => {
        'name': name,
        'ty': ty.toJson(),
        'label': label,
        'description': description,
      };
}

class ActionMetadata {
  const ActionMetadata({
    required this.functionName,
    this.description,
    required this.parameters,
    required this.results,
  });

  factory ActionMetadata.fromJson(Map<String, Object?> json) => ActionMetadata(
        functionName: json['function_name'] as String,
        description: json['description'] as String?,
        parameters: (json['parameters'] as List)
            .map((p) => ActionParameterMetadata.fromJson(p as Map<String, Object?>))
            .toList(),
        results: (json['results'] as List)
            .map((r) => ActionResultMetadata.fromJson(r as Map<String, Object?>))
            .toList(),
      );

  final String functionName;
  final String? description;
  final List<ActionParameterMetadata> parameters;
  final List<ActionResultMetadata> results;

  Map<String, Object?> toJson() => {
        'function_name': functionName,
        'description': description,
        'parameters': parameters.map((p) => p.toJson()).toList(),
        'results': results.map((r) => r.toJson()).toList(),
      };
}
