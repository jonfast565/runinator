// json value algebra mirroring runinator-models::Value. use at api/persistence
// boundaries; narrow into domain structs (workflow state, gate rows, etc.) at read time.
//
// dart's json.decode already produces null/bool/num/String/List/Map, so JsonValue
// is a plain Object? alias rather than a sealed type; the helpers below narrow it
// the same way the ts asJson*/isJson* functions do.

typedef JsonArray = List<Object?>;

typedef JsonObject = Map<String, Object?>;

/// mutable editor/wire object map, same shape as [JsonObject] in dart. kept as a
/// separate alias to preserve the ts source's naming (JsonRecord vs JsonObject).
typedef JsonRecord = Map<String, Object?>;

typedef JsonValue = Object?;

bool isJsonObject(JsonValue value) => value is Map<String, Object?>;

bool isJsonArray(JsonValue value) => value is List;

bool isJsonRecord(Object? value) => value is Map<String, Object?>;

JsonObject asJsonObject(JsonValue value) =>
    isJsonObject(value) ? value as JsonObject : <String, Object?>{};

JsonArray asJsonArray(JsonValue value) =>
    isJsonArray(value) ? (value as List).cast<Object?>() : <Object?>[];

/// narrow an unknown wire/editor value to [JsonValue] for assignment into typed json fields.
JsonValue asJsonValue(Object? value) {
  if (value == null || value is String || value is num || value is bool) {
    return value;
  }

  if (value is List) {
    return value.cast<Object?>();
  }

  if (value is Map) {
    return value.map((key, v) => MapEntry(key.toString(), v));
  }

  return null;
}

JsonRecord asJsonRecord(Object? value) =>
    isJsonRecord(value) ? value as JsonRecord : <String, Object?>{};
