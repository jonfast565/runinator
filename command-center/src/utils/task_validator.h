#pragma once

#include "models/scheduled_task.h"

inline QString validateTask(const ScheduledTask &task) {
  if (task.name.trimmed().isEmpty()) {
    return "Name is required";
  }
  if (task.cronSchedule.trimmed().isEmpty()) {
    return "Cron is required";
  }
  if (task.actionName.trimmed().isEmpty()) {
    return "Action name is required";
  }
  if (task.actionFunction.trimmed().isEmpty()) {
    return "Action function is required";
  }
  if (task.actionConfiguration.trimmed().isEmpty()) {
    return "Action configuration is required";
  }
  if (task.timeout <= 0) {
    return "Timeout must be > 0";
  }
  return QString();
}
