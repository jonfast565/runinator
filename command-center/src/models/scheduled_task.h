#pragma once

#include <QDateTime>
#include <QJsonObject>
#include <QJsonArray>
#include <QString>
#include <optional>

struct ScheduledTask {
  std::optional<qint64> id;
  QString name;
  QString cronSchedule;
  QString actionName;
  QString actionFunction;
  QString actionConfiguration;
  qint64 timeout = 0;
  std::optional<QDateTime> nextExecution;
  bool enabled = true;
  bool immediate = false;
  std::optional<QDateTime> blackoutStart;
  std::optional<QDateTime> blackoutEnd;
  QJsonObject inputSchema;
  QJsonObject defaultParameters;
  QJsonObject outputSchema;
  bool hasOutputSchema = false;
  bool mcpEnabled = false;
  QJsonObject metadata;
  QStringList tags;

  static std::optional<QDateTime> parseOptionalDate(const QJsonValue &value);
  static ScheduledTask fromJson(const QJsonObject &obj);
  static QJsonValue dateOrNull(const std::optional<QDateTime> &dt);

  QJsonObject toJson() const;
};

QString formatDate(const std::optional<QDateTime> &dt);
