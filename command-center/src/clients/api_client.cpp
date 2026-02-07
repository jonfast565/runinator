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
  QString basePath = base.path();
  if (!basePath.endsWith('/')) {
    basePath += "/";
  }
  base.setPath(basePath + trimmed);
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
