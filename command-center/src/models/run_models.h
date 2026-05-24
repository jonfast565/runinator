#pragma once

#include <QDateTime>
#include <QJsonObject>
#include <QVector>
#include <QString>
#include <optional>

struct RunSummary {
  qint64 id = 0;
  qint64 taskId = 0;
  QString status;
  QJsonObject parameters;
  QJsonObject outputJson;
  QString message;
  QString trigger;
  std::optional<QDateTime> startedAt;
  std::optional<QDateTime> finishedAt;
  QDateTime createdAt;
  std::optional<qint64> workflowRunId;
  QString workflowNodeId;

  static RunSummary fromJson(const QJsonObject &obj);
};

struct RunChunk {
  qint64 id = 0;
  qint64 runId = 0;
  qint64 sequence = 0;
  QString stream;
  QString content;
  QDateTime createdAt;

  static RunChunk fromJson(const QJsonObject &obj);
};

struct RunArtifact {
  qint64 id = 0;
  qint64 runId = 0;
  QString name;
  QString mimeType;
  qint64 sizeBytes = 0;
  QString uri;
  QJsonObject metadata;
  QDateTime createdAt;

  static RunArtifact fromJson(const QJsonObject &obj);
};

struct WorkflowDefinition {
  std::optional<qint64> id;
  QString name;
  qint64 version = 1;
  bool enabled = true;
  QJsonObject inputType;
  QJsonObject definition;

  static WorkflowDefinition fromJson(const QJsonObject &obj);
  QJsonObject toJson() const;
};

struct WorkflowNodeRun {
  qint64 id = 0;
  qint64 workflowRunId = 0;
  QString nodeId;
  std::optional<qint64> taskRunId;
  QString status;
  qint64 attempt = 0;
  QJsonObject parameters;
  QJsonObject outputJson;
  QJsonObject state;
  QString transitionReason;
  QDateTime createdAt;
  std::optional<QDateTime> startedAt;
  std::optional<QDateTime> finishedAt;
  QString message;

  static WorkflowNodeRun fromJson(const QJsonObject &obj);
};

struct WorkflowRunDetail {
  qint64 id = 0;
  qint64 workflowId = 0;
  QString status;
  QJsonObject parameters;
  QDateTime createdAt;
  std::optional<QDateTime> startedAt;
  std::optional<QDateTime> finishedAt;
  QString message;
  QVector<WorkflowNodeRun> nodes;

  static WorkflowRunDetail fromJson(const QJsonObject &obj);
};

struct WorkflowRunSummary {
  qint64 id = 0;
  qint64 workflowId = 0;
  QString status;
  QJsonObject parameters;
  QDateTime createdAt;
  std::optional<QDateTime> startedAt;
  std::optional<QDateTime> finishedAt;
  QString message;

  static WorkflowRunSummary fromJson(const QJsonObject &obj);
};

QString formatDateTime(const QDateTime &dt);
QString formatOptionalDateTime(const std::optional<QDateTime> &dt);
