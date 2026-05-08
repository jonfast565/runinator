#include "scheduled_task.h"

#include <QJsonArray>
#include <QJsonValue>

namespace {
QJsonObject objectOrDefault(const QJsonValue &value, const QJsonObject &fallback = QJsonObject()) {
  return value.isObject() ? value.toObject() : fallback;
}

QJsonObject defaultInputSchema() {
  QJsonObject schema;
  schema.insert("type", "object");
  schema.insert("additionalProperties", true);
  return schema;
}
} // namespace

std::optional<QDateTime> ScheduledTask::parseOptionalDate(const QJsonValue &value) {
  if (!value.isString()) {
    return std::nullopt;
  }
  const QString text = value.toString();
  if (text.trimmed().isEmpty()) {
    return std::nullopt;
  }
  QDateTime dt = QDateTime::fromString(text, Qt::ISODateWithMs);
  if (!dt.isValid()) {
    dt = QDateTime::fromString(text, Qt::ISODate);
  }
  if (!dt.isValid()) {
    return std::nullopt;
  }
  dt.setTimeSpec(Qt::UTC);
  return dt;
}

ScheduledTask ScheduledTask::fromJson(const QJsonObject &obj) {
  ScheduledTask task;
  if (obj.contains("id") && !obj.value("id").isNull()) {
    task.id = obj.value("id").toVariant().toLongLong();
  }
  task.name = obj.value("name").toString();
  task.cronSchedule = obj.value("cron_schedule").toString();
  task.actionName = obj.value("action_name").toString();
  task.actionFunction = obj.value("action_function").toString();
  task.actionConfiguration = obj.value("action_configuration").toString();
  task.timeout = obj.value("timeout").toVariant().toLongLong();
  task.nextExecution = parseOptionalDate(obj.value("next_execution"));
  task.enabled = obj.value("enabled").toBool(true);
  task.immediate = obj.value("immediate").toBool(false);
  task.blackoutStart = parseOptionalDate(obj.value("blackout_start"));
  task.blackoutEnd = parseOptionalDate(obj.value("blackout_end"));
  task.inputSchema = objectOrDefault(obj.value("input_schema"), defaultInputSchema());
  task.defaultParameters = objectOrDefault(obj.value("default_parameters"));
  task.hasOutputSchema = obj.contains("output_schema") && obj.value("output_schema").isObject();
  task.outputSchema = objectOrDefault(obj.value("output_schema"));
  task.mcpEnabled = obj.value("mcp_enabled").toBool(false);
  task.metadata = objectOrDefault(obj.value("metadata"));
  if (obj.value("tags").isArray()) {
    for (const auto &tag : obj.value("tags").toArray()) {
      task.tags.push_back(tag.toString());
    }
  }
  return task;
}

QJsonValue ScheduledTask::dateOrNull(const std::optional<QDateTime> &dt) {
  if (!dt.has_value()) {
    return QJsonValue::Null;
  }
  return QJsonValue(dt.value().toUTC().toString(Qt::ISODateWithMs));
}

QJsonObject ScheduledTask::toJson() const {
  QJsonObject obj;
  if (id.has_value()) {
    obj.insert("id", static_cast<double>(id.value()));
  } else {
    obj.insert("id", QJsonValue::Null);
  }
  obj.insert("name", name);
  obj.insert("cron_schedule", cronSchedule);
  obj.insert("action_name", actionName);
  obj.insert("action_function", actionFunction);
  obj.insert("action_configuration", actionConfiguration);
  obj.insert("timeout", static_cast<double>(timeout));
  obj.insert("next_execution", dateOrNull(nextExecution));
  obj.insert("enabled", enabled);
  obj.insert("immediate", immediate);
  obj.insert("blackout_start", dateOrNull(blackoutStart));
  obj.insert("blackout_end", dateOrNull(blackoutEnd));
  obj.insert("input_schema", inputSchema.isEmpty() ? defaultInputSchema() : inputSchema);
  obj.insert("default_parameters", defaultParameters);
  obj.insert("output_schema", hasOutputSchema ? QJsonValue(outputSchema) : QJsonValue::Null);
  obj.insert("mcp_enabled", mcpEnabled);
  obj.insert("metadata", metadata);
  QJsonArray tagArray;
  for (const auto &tag : tags) {
    tagArray.append(tag);
  }
  obj.insert("tags", tagArray);
  return obj;
}

QString formatDate(const std::optional<QDateTime> &dt) {
  if (!dt.has_value()) {
    return "-";
  }
  return dt.value().toUTC().toString("yyyy-MM-dd HH:mm:ss");
}
