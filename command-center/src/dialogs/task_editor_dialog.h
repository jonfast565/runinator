#pragma once

#include <QDialog>
#include <QShortcut>

#include "models/scheduled_task.h"

namespace Ui {
class TaskEditorDialog;
}

class TaskEditorDialog : public QDialog {
  Q_OBJECT
public:
  explicit TaskEditorDialog(QWidget *parent = nullptr);
  ~TaskEditorDialog() override;

  void setTask(const ScheduledTask &task, bool creatingTask);
  void setSaving(bool saving);
  void setError(const QString &message);

signals:
  void saveRequested(const ScheduledTask &task, bool creating);

private:
  ScheduledTask collectTask() const;
  void handleSave();

  Ui::TaskEditorDialog *ui = nullptr;
  bool creating = true;
  std::optional<qint64> taskId;
  std::optional<QDateTime> nextExecution;
  bool immediate = false;
  std::optional<QDateTime> blackoutStart;
  std::optional<QDateTime> blackoutEnd;
};
