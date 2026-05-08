#pragma once

#include <QNetworkAccessManager>
#include <QObject>
#include <QUrl>
#include <QVector>

#include "models/run_models.h"
#include "models/scheduled_task.h"

class ApiClient : public QObject {
  Q_OBJECT
public:
  explicit ApiClient(QObject *parent = nullptr);

  void setBaseUrl(const QString &baseUrl);
  QString baseUrl() const;

  void fetchTasks();
  void requestRun(qint64 taskId);
  void createTask(const ScheduledTask &task);
  void updateTask(const ScheduledTask &task);
  void fetchRuns(qint64 taskId);
  void fetchRunChunks(qint64 runId);
  void fetchRunArtifacts(qint64 runId);
  void fetchWorkflows();
  void saveWorkflow(const WorkflowDefinition &workflow);
  void createWorkflowRun(qint64 workflowId);
  void fetchWorkflowRun(qint64 workflowRunId);
  void fetchWorkflowRuns(qint64 workflowId);

signals:
  void tasksLoaded(const QVector<ScheduledTask> &tasks);
  void runsLoaded(const QVector<RunSummary> &runs);
  void runChunksLoaded(qint64 runId, const QVector<RunChunk> &chunks);
  void runArtifactsLoaded(qint64 runId, const QVector<RunArtifact> &artifacts);
  void workflowsLoaded(const QVector<WorkflowDefinition> &workflows);
  void workflowSaved(const WorkflowDefinition &workflow);
  void workflowRunsLoaded(qint64 workflowId, const QVector<WorkflowRunSummary> &runs);
  void workflowRunLoaded(const WorkflowRunDetail &detail);
  void requestFailed(const QString &message);
  void taskRunResult(bool ok, const QString &message);
  void taskSaved(bool ok, const QString &message, bool creating);
  void workflowRunRequested(qint64 workflowRunId);

private:
  QUrl buildUrl(const QString &path) const;
  static bool parseTaskResponse(const QByteArray &body, bool &ok, QString &message);
  static QString extractError(const QByteArray &body, QNetworkReply *reply);

  QNetworkAccessManager *network = nullptr;
  QString serviceBaseUrl;
};
