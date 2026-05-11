#include "api_client.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkReply>

ApiClient::ApiClient(QObject *parent) : QObject(parent) {
  network = new QNetworkAccessManager(this);
}

void ApiClient::setBaseUrl(const QString &baseUrl) { serviceBaseUrl = baseUrl; }

QString ApiClient::baseUrl() const { return serviceBaseUrl; }

QUrl ApiClient::buildUrl(const QString &path) const {
  QUrl base(serviceBaseUrl);
  QString trimmed = path;
  if (trimmed.startsWith('/')) {
    trimmed.remove(0, 1);
  }
  const int queryStart = trimmed.indexOf('?');
  QString query;
  if (queryStart >= 0) {
    query = trimmed.mid(queryStart + 1);
    trimmed = trimmed.left(queryStart);
  }
  QString basePath = base.path();
  if (!basePath.endsWith('/')) {
    basePath += "/";
  }
  base.setPath(basePath + trimmed);
  if (!query.isEmpty()) {
    base.setQuery(query);
  }
  return base;
}

void ApiClient::fetchTasks() {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  QNetworkRequest request(buildUrl("tasks"));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError) {
      emit requestFailed("Failed to parse task list");
      return;
    }

    if (!doc.isArray()) {
      if (doc.isObject()) {
        const QString message = doc.object().value("message").toString();
        if (!message.isEmpty()) {
          emit requestFailed(message);
          return;
        }
      }
      emit requestFailed("Unexpected response from service");
      return;
    }

    QVector<ScheduledTask> loaded;
    for (const auto &item : doc.array()) {
      if (item.isObject()) {
        loaded.push_back(ScheduledTask::fromJson(item.toObject()));
      }
    }

    emit tasksLoaded(loaded);
  });
}

void ApiClient::requestRun(qint64 taskId) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  QNetworkRequest request(buildUrl(QString("tasks/%1/request_run").arg(taskId)));
  request.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");
  QNetworkReply *reply = network->post(request, QByteArray());
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    bool ok = false;
    QString message;
    if (!parseTaskResponse(body, ok, message)) {
      emit requestFailed("Unexpected response from service");
      return;
    }

    emit taskRunResult(ok, message);
  });
}

void ApiClient::createTask(const ScheduledTask &task) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  const QByteArray payload = QJsonDocument(task.toJson()).toJson(QJsonDocument::Compact);
  QNetworkRequest request(buildUrl("tasks"));
  request.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");

  QNetworkReply *reply = network->post(request, payload);
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    bool ok = false;
    QString message;
    if (!parseTaskResponse(body, ok, message)) {
      emit requestFailed("Unexpected response from service");
      return;
    }

    emit taskSaved(ok, message, true);
  });
}

void ApiClient::updateTask(const ScheduledTask &task) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  if (!task.id.has_value()) {
    emit requestFailed("Task is missing an ID");
    return;
  }

  const QByteArray payload = QJsonDocument(task.toJson()).toJson(QJsonDocument::Compact);
  QNetworkRequest request(buildUrl(QString("tasks/%1").arg(task.id.value())));
  request.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");

  QNetworkReply *reply = network->sendCustomRequest(request, "PATCH", payload);
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    bool ok = false;
    QString message;
    if (!parseTaskResponse(body, ok, message)) {
      emit requestFailed("Unexpected response from service");
      return;
    }

    emit taskSaved(ok, message, false);
  });
}

void ApiClient::fetchRuns(qint64 taskId) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  QNetworkRequest request(buildUrl(QString("tasks/%1/runs").arg(taskId)));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isArray()) {
      emit requestFailed("Failed to parse run list");
      return;
    }

    QVector<RunSummary> loaded;
    for (const auto &item : doc.array()) {
      if (item.isObject()) {
        loaded.push_back(RunSummary::fromJson(item.toObject()));
      }
    }
    emit runsLoaded(loaded);
  });
}

void ApiClient::fetchRunChunks(qint64 runId) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  QNetworkRequest request(buildUrl(QString("runs/%1/chunks?limit=500").arg(runId)));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply, runId]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isArray()) {
      emit requestFailed("Failed to parse run chunks");
      return;
    }

    QVector<RunChunk> loaded;
    for (const auto &item : doc.array()) {
      if (item.isObject()) {
        loaded.push_back(RunChunk::fromJson(item.toObject()));
      }
    }
    emit runChunksLoaded(runId, loaded);
  });
}

void ApiClient::fetchRunArtifacts(qint64 runId) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  QNetworkRequest request(buildUrl(QString("runs/%1/artifacts").arg(runId)));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply, runId]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isArray()) {
      emit requestFailed("Failed to parse run artifacts");
      return;
    }

    QVector<RunArtifact> loaded;
    for (const auto &item : doc.array()) {
      if (item.isObject()) {
        loaded.push_back(RunArtifact::fromJson(item.toObject()));
      }
    }
    emit runArtifactsLoaded(runId, loaded);
  });
}

void ApiClient::fetchWorkflows() {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  QNetworkRequest request(buildUrl("workflows"));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isArray()) {
      emit requestFailed("Failed to parse workflow list");
      return;
    }

    QVector<WorkflowDefinition> loaded;
    for (const auto &item : doc.array()) {
      if (item.isObject()) {
        loaded.push_back(WorkflowDefinition::fromJson(item.toObject()));
      }
    }
    emit workflowsLoaded(loaded);
  });
}

void ApiClient::saveWorkflow(const WorkflowDefinition &workflow) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  const QByteArray payload = QJsonDocument(workflow.toJson()).toJson(QJsonDocument::Compact);
  QNetworkRequest request(workflow.id.has_value()
                              ? buildUrl(QString("workflows/%1").arg(workflow.id.value()))
                              : buildUrl("workflows"));
  request.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");

  QNetworkReply *reply = workflow.id.has_value()
                             ? network->sendCustomRequest(request, "PATCH", payload)
                             : network->post(request, payload);
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
      emit requestFailed("Failed to parse saved workflow");
      return;
    }
    emit workflowSaved(WorkflowDefinition::fromJson(doc.object()));
  });
}

void ApiClient::createWorkflowRun(qint64 workflowId) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("No service discovered");
    return;
  }

  QNetworkRequest request(buildUrl(QString("workflows/%1/runs").arg(workflowId)));
  request.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");
  QNetworkReply *reply = network->post(request, QByteArray("{}"));
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
      emit requestFailed("Failed to parse workflow run response");
      return;
    }

    const QJsonObject runObj = doc.object().value("run").toObject();
    emit workflowRunRequested(runObj.value("id").toVariant().toLongLong());
  });
}

void ApiClient::fetchWorkflowRun(qint64 workflowRunId) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("API base URL is not configured");
    return;
  }

  QNetworkRequest request(buildUrl(QString("workflow_runs/%1").arg(workflowRunId)));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
      emit requestFailed("Failed to parse workflow run detail");
      return;
    }

    emit workflowRunLoaded(WorkflowRunDetail::fromJson(doc.object()));
  });
}

void ApiClient::fetchWorkflowRuns(qint64 workflowId) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("API base URL is not configured");
    return;
  }

  QNetworkRequest request(buildUrl(QString("workflow_runs?workflow_id=%1").arg(workflowId)));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply, workflowId]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isArray()) {
      emit requestFailed("Failed to parse workflow run history");
      return;
    }

    QVector<WorkflowRunSummary> loaded;
    for (const auto &item : doc.array()) {
      if (item.isObject()) {
        loaded.push_back(WorkflowRunSummary::fromJson(item.toObject()));
      }
    }
    emit workflowRunsLoaded(workflowId, loaded);
  });
}

void ApiClient::fetchGenericRecords(const QString &endpoint) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("API base URL is not configured");
    return;
  }

  QNetworkRequest request(buildUrl(endpoint));
  QNetworkReply *reply = network->get(request);
  connect(reply, &QNetworkReply::finished, this, [this, reply, endpoint]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit requestFailed(extractError(body, reply));
      return;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isArray()) {
      emit requestFailed(QString("Failed to parse %1").arg(endpoint));
      return;
    }

    QVector<QJsonObject> loaded;
    for (const auto &item : doc.array()) {
      if (item.isObject()) {
        loaded.push_back(item.toObject());
      }
    }
    emit genericRecordsLoaded(endpoint, loaded);
  });
}

void ApiClient::approveApproval(qint64 approvalId) {
  postApprovalAction(approvalId, "approve");
}

void ApiClient::rejectApproval(qint64 approvalId) {
  postApprovalAction(approvalId, "reject");
}

void ApiClient::postApprovalAction(qint64 approvalId, const QString &action) {
  if (serviceBaseUrl.isEmpty()) {
    emit requestFailed("API base URL is not configured");
    return;
  }

  QNetworkRequest request(buildUrl(QString("approvals/%1/%2").arg(approvalId).arg(action)));
  request.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");
  QNetworkReply *reply = network->post(request, QByteArray("{}"));
  connect(reply, &QNetworkReply::finished, this, [this, reply, action]() {
    const QByteArray body = reply->readAll();
    const bool hasError = reply->error() != QNetworkReply::NoError;
    reply->deleteLater();

    if (hasError) {
      emit approvalActionFinished(false, extractError(body, reply));
      return;
    }

    QString message = QString("Approval %1d").arg(action);
    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error == QJsonParseError::NoError && doc.isObject()) {
      const QString parsed = doc.object().value("message").toString();
      if (!parsed.isEmpty()) {
        message = parsed;
      }
    }
    emit approvalActionFinished(true, message);
  });
}

bool ApiClient::parseTaskResponse(const QByteArray &body, bool &ok, QString &message) {
  QJsonParseError parseError;
  const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
  if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
    return false;
  }
  const QJsonObject obj = doc.object();
  if (!obj.contains("success") || !obj.contains("message")) {
    return false;
  }
  ok = obj.value("success").toBool();
  message = obj.value("message").toString();
  return true;
}

QString ApiClient::extractError(const QByteArray &body, QNetworkReply *reply) {
  if (!body.isEmpty()) {
    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(body, &parseError);
    if (parseError.error == QJsonParseError::NoError && doc.isObject()) {
      const QString msg = doc.object().value("message").toString();
      if (!msg.isEmpty()) {
        return msg;
      }
    }
    const QString raw = QString::fromUtf8(body).trimmed();
    if (!raw.isEmpty()) {
      return raw;
    }
  }
  return reply->errorString();
}
