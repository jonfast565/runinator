// port of core/domain/models/provider/runinator-type.ts.
//
// unlike most models in this package, RuninatorType is a genuinely tagged union
// whose variants carry different payloads per tag, so it is modeled as a sealed
// class hierarchy rather than a plain nullable-field record.

import '../../json.dart';

sealed class RuninatorType {
  const RuninatorType();

  static RuninatorType fromJson(Map<String, Object?> json) {
    final type = json['type'];
    return switch (type) {
      'null' => const RuninatorTypeNull(),
      'boolean' => const RuninatorTypeBoolean(),
      'integer' => const RuninatorTypeInteger(),
      'number' => const RuninatorTypeNumber(),
      'duration' => const RuninatorTypeDuration(),
      'string' => const RuninatorTypeString(),
      'enum' => RuninatorTypeEnum.fromJson(json),
      'range' => RuninatorTypeRange.fromJson(json),
      'array' => RuninatorTypeArray.fromJson(json),
      'map' => RuninatorTypeMap.fromJson(json),
      'struct' => RuninatorTypeStruct.fromJson(json),
      'union' => RuninatorTypeUnion.fromJson(json),
      'any' => const RuninatorTypeAny(),
      _ => throw ArgumentError('unknown RuninatorType: $type'),
    };
  }

  /// narrow workflow input_type from the wire definition blob.
  static RuninatorType? tryParse(JsonValue value) {
    if (!isJsonObject(value)) {
      return null;
    }

    final type = asJsonObject(value)['type'];
    if (type is! String) {
      return null;
    }

    return RuninatorType.fromJson(asJsonObject(value));
  }

  Map<String, Object?> toJson();
}

class RuninatorTypeNull extends RuninatorType {
  const RuninatorTypeNull();

  @override
  Map<String, Object?> toJson() => {'type': 'null'};
}

class RuninatorTypeBoolean extends RuninatorType {
  const RuninatorTypeBoolean();

  @override
  Map<String, Object?> toJson() => {'type': 'boolean'};
}

class RuninatorTypeInteger extends RuninatorType {
  const RuninatorTypeInteger();

  @override
  Map<String, Object?> toJson() => {'type': 'integer'};
}

class RuninatorTypeNumber extends RuninatorType {
  const RuninatorTypeNumber();

  @override
  Map<String, Object?> toJson() => {'type': 'number'};
}

class RuninatorTypeDuration extends RuninatorType {
  const RuninatorTypeDuration();

  @override
  Map<String, Object?> toJson() => {'type': 'duration'};
}

class RuninatorTypeString extends RuninatorType {
  const RuninatorTypeString();

  @override
  Map<String, Object?> toJson() => {'type': 'string'};
}

class RuninatorTypeAny extends RuninatorType {
  const RuninatorTypeAny();

  @override
  Map<String, Object?> toJson() => {'type': 'any'};
}

class RuninatorTypeEnum extends RuninatorType {
  const RuninatorTypeEnum(this.values);

  factory RuninatorTypeEnum.fromJson(Map<String, Object?> json) =>
      RuninatorTypeEnum(asJsonArray(json['values']));

  final JsonArray values;

  @override
  Map<String, Object?> toJson() => {'type': 'enum', 'values': values};
}

class RuninatorTypeRange extends RuninatorType {
  const RuninatorTypeRange({required this.base, this.min, this.max});

  factory RuninatorTypeRange.fromJson(Map<String, Object?> json) => RuninatorTypeRange(
        base: RuninatorType.fromJson(json['base'] as Map<String, Object?>),
        min: (json['min'] as num?)?.toDouble(),
        max: (json['max'] as num?)?.toDouble(),
      );

  final RuninatorType base;
  final double? min;
  final double? max;

  @override
  Map<String, Object?> toJson() => {
        'type': 'range',
        'base': base.toJson(),
        if (min != null) 'min': min,
        if (max != null) 'max': max,
      };
}

class RuninatorTypeArray extends RuninatorType {
  const RuninatorTypeArray(this.items);

  factory RuninatorTypeArray.fromJson(Map<String, Object?> json) =>
      RuninatorTypeArray(RuninatorType.fromJson(json['items'] as Map<String, Object?>));

  final RuninatorType items;

  @override
  Map<String, Object?> toJson() => {'type': 'array', 'items': items.toJson()};
}

class RuninatorTypeMap extends RuninatorType {
  const RuninatorTypeMap(this.values);

  factory RuninatorTypeMap.fromJson(Map<String, Object?> json) =>
      RuninatorTypeMap(RuninatorType.fromJson(json['values'] as Map<String, Object?>));

  final RuninatorType values;

  @override
  Map<String, Object?> toJson() => {'type': 'map', 'values': values.toJson()};
}

class RuninatorField {
  const RuninatorField({required this.ty, required this.required});

  factory RuninatorField.fromJson(Map<String, Object?> json) => RuninatorField(
        ty: RuninatorType.fromJson(json['ty'] as Map<String, Object?>),
        required: json['required'] as bool,
      );

  final RuninatorType ty;
  final bool required;

  Map<String, Object?> toJson() => {'ty': ty.toJson(), 'required': required};
}

class RuninatorTypeStruct extends RuninatorType {
  const RuninatorTypeStruct({required this.fields, this.additional});

  factory RuninatorTypeStruct.fromJson(Map<String, Object?> json) => RuninatorTypeStruct(
        fields: (json['fields'] as Map<String, Object?>).map(
          (key, value) => MapEntry(key, RuninatorField.fromJson(value as Map<String, Object?>)),
        ),
        additional: json['additional'] != null
            ? RuninatorType.fromJson(json['additional'] as Map<String, Object?>)
            : null,
      );

  final Map<String, RuninatorField> fields;
  final RuninatorType? additional;

  @override
  Map<String, Object?> toJson() => {
        'type': 'struct',
        'fields': fields.map((key, value) => MapEntry(key, value.toJson())),
        if (additional != null) 'additional': additional!.toJson(),
      };
}

class RuninatorTypeUnion extends RuninatorType {
  const RuninatorTypeUnion(this.variants);

  factory RuninatorTypeUnion.fromJson(Map<String, Object?> json) => RuninatorTypeUnion(
        (json['variants'] as List)
            .map((variant) => RuninatorType.fromJson(variant as Map<String, Object?>))
            .toList(),
      );

  final List<RuninatorType> variants;

  @override
  Map<String, Object?> toJson() => {
        'type': 'union',
        'variants': variants.map((variant) => variant.toJson()).toList(),
      };
}
