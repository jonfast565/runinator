#pragma once

#include <QNetworkAccessManager>
#include <QObject>
#include <QUrl>
#include <QVector>

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

signals:
  void tasksLoaded(const QVector<ScheduledTask> &tasks);
  void requestFailed(const QString &message);
  void taskRunResult(bool ok, const QString &message);
  void taskSaved(bool ok, const QString &message, bool creating);

private:
  QUrl buildUrl(const QString &path) const;
  static bool parseTaskResponse(const QByteArray &body, bool &ok, QString &message);
  static QString extractError(const QByteArray &body, QNetworkReply *reply);

  QNetworkAccessManager *network = nullptr;
  QString serviceBaseUrl;
};
