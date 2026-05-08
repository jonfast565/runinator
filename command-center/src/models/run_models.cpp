#include "run_models.h"

#include <QJsonArray>
#include <QJsonValue>
#include <QVector>

namespace {
std::optional<QDateTime> parseOptionalDate(const QJsonValue &value) {
  if (!value.isString()) {
    return std::nullopt;
  }
  QDateTime dt = QDateTime::fromString(value.toString(), Qt::ISODateWithMs);
  if (!dt.isValid()) {
    dt = QDateTime::fromString(value.toString(), Qt::ISODate);
  }
  if (!dt.isValid()) {
    return std::nullopt;
  }
  dt.setTimeSpec(Qt::UTC);
  return dt;
}

QDateTime parseRequiredDate(const QJsonValue &value) {
  return parseOptionalDate(value).value_or(QDateTime());
}

QJsonObject objectOrEmpty(const QJsonValue &value) {
  return value.isObject() ? value.toObject() : QJsonObject();
}
} // namespace

RunSummary RunSummary::fromJson(const QJsonObject &obj) {
  RunSummary run;
  run.id = obj.value("id").toVariant().toLongLong();
  run.taskId = obj.value("task_id").toVariant().toLongLong();
  run.status = obj.value("status").toString();
  run.parameters = objectOrEmpty(obj.value("parameters"));
  run.outputJson = objectOrEmpty(obj.value("output_json"));
  run.message = obj.value("message").toString();
  run.trigger = obj.value("trigger").toString();
  run.startedAt = parseOptionalDate(obj.value("started_at"));
  run.finishedAt = parseOptionalDate(obj.value("finished_at"));
  run.createdAt = parseRequiredDate(obj.value("created_at"));
  if (obj.contains("workflow_run_id") && !obj.value("workflow_run_id").isNull()) {
    run.workflowRunId = obj.value("workflow_run_id").toVariant().toLongLong();
  }
  run.workflowStepId = obj.value("workflow_step_id").toString();
  return run;
}

RunChunk RunChunk::fromJson(const QJsonObject &obj) {
  RunChunk chunk;
  chunk.id = obj.value("id").toVariant().toLongLong();
  chunk.runId = obj.value("run_id").toVariant().toLongLong();
  chunk.sequence = obj.value("sequence").toVariant().toLongLong();
  chunk.stream = obj.value("stream").toString();
  chunk.content = obj.value("content").toString();
  chunk.createdAt = parseRequiredDate(obj.value("created_at"));
  return chunk;
}

RunArtifact RunArtifact::fromJson(const QJsonObject &obj) {
  RunArtifact artifact;
  artifact.id = obj.value("id").toVariant().toLongLong();
  artifact.runId = obj.value("run_id").toVariant().toLongLong();
  artifact.name = obj.value("name").toString();
  artifact.mimeType = obj.value("mime_type").toString();
  artifact.sizeBytes = obj.value("size_bytes").toVariant().toLongLong();
  artifact.uri = obj.value("uri").toString();
  artifact.metadata = objectOrEmpty(obj.value("metadata"));
  artifact.createdAt = parseRequiredDate(obj.value("created_at"));
  return artifact;
}

WorkflowDefinition WorkflowDefinition::fromJson(const QJsonObject &obj) {
  WorkflowDefinition workflow;
  if (obj.contains("id") && !obj.value("id").isNull()) {
    workflow.id = obj.value("id").toVariant().toLongLong();
  }
  workflow.name = obj.value("name").toString();
  workflow.version = obj.value("version").toVariant().toLongLong();
  workflow.enabled = obj.value("enabled").toBool(true);
  workflow.inputSchema = objectOrEmpty(obj.value("input_schema"));
  workflow.definition = objectOrEmpty(obj.value("definition"));
  return workflow;
}

QJsonObject WorkflowDefinition::toJson() const {
  QJsonObject obj;
  obj.insert("id", id.has_value() ? QJsonValue(static_cast<double>(id.value())) : QJsonValue::Null);
  obj.insert("name", name);
  obj.insert("version", static_cast<double>(version));
  obj.insert("enabled", enabled);
  obj.insert("input_schema", inputSchema);
  obj.insert("definition", definition);
  return obj;
}

WorkflowStepRun WorkflowStepRun::fromJson(const QJsonObject &obj) {
  WorkflowStepRun step;
  step.id = obj.value("id").toVariant().toLongLong();
  step.workflowRunId = obj.value("workflow_run_id").toVariant().toLongLong();
  step.stepId = obj.value("step_id").toString();
  if (obj.contains("task_run_id") && !obj.value("task_run_id").isNull()) {
    step.taskRunId = obj.value("task_run_id").toVariant().toLongLong();
  }
  step.status = obj.value("status").toString();
  step.attempt = obj.value("attempt").toVariant().toLongLong();
  step.parameters = objectOrEmpty(obj.value("parameters"));
  step.createdAt = parseRequiredDate(obj.value("created_at"));
  step.startedAt = parseOptionalDate(obj.value("started_at"));
  step.finishedAt = parseOptionalDate(obj.value("finished_at"));
  step.message = obj.value("message").toString();
  return step;
}

WorkflowRunDetail WorkflowRunDetail::fromJson(const QJsonObject &obj) {
  WorkflowRunDetail detail;
  const QJsonObject run = obj.value("run").toObject();
  detail.id = run.value("id").toVariant().toLongLong();
  detail.workflowId = run.value("workflow_id").toVariant().toLongLong();
  detail.status = run.value("status").toString();
  detail.parameters = objectOrEmpty(run.value("parameters"));
  detail.createdAt = parseRequiredDate(run.value("created_at"));
  detail.startedAt = parseOptionalDate(run.value("started_at"));
  detail.finishedAt = parseOptionalDate(run.value("finished_at"));
  detail.message = run.value("message").toString();
  for (const auto &value : obj.value("steps").toArray()) {
    if (value.isObject()) {
      detail.steps.push_back(WorkflowStepRun::fromJson(value.toObject()));
    }
  }
  return detail;
}

WorkflowRunSummary WorkflowRunSummary::fromJson(const QJsonObject &obj) {
  WorkflowRunSummary run;
  run.id = obj.value("id").toVariant().toLongLong();
  run.workflowId = obj.value("workflow_id").toVariant().toLongLong();
  run.status = obj.value("status").toString();
  run.parameters = objectOrEmpty(obj.value("parameters"));
  run.createdAt = parseRequiredDate(obj.value("created_at"));
  run.startedAt = parseOptionalDate(obj.value("started_at"));
  run.finishedAt = parseOptionalDate(obj.value("finished_at"));
  run.message = obj.value("message").toString();
  return run;
}

QString formatDateTime(const QDateTime &dt) {
  if (!dt.isValid()) {
    return "-";
  }
  return dt.toUTC().toString("yyyy-MM-dd HH:mm:ss");
}

QString formatOptionalDateTime(const std::optional<QDateTime> &dt) {
  return dt.has_value() ? formatDateTime(dt.value()) : "-";
}
