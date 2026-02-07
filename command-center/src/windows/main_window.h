#pragma once

#include <QLabel>
#include <QMainWindow>
#include <QStandardItemModel>
#include <QTimer>
#include <QStringList>

#include "clients/api_client.h"
#include "dialogs/task_editor_dialog.h"
#include "gossip/gossip_discovery.h"
#include "models/scheduled_task.h"

namespace Ui {
class MainWindow;
}

class MainWindow : public QMainWindow {
  Q_OBJECT
public:
  explicit MainWindow(QWidget *parent = nullptr);
  ~MainWindow() override;

private:
  void setupUiBindings();
  void setupShortcuts();
  void refreshTasks();
  void updateTable();
  void updateRunNowState();
  void requestRunSelected();
  void addNewTask();
  void editSelectedTask();
  void openEditor(const ScheduledTask &task, bool creating);
  void submitTask(const ScheduledTask &task, bool creating);
  void setStatus(const QString &text);
  void setError(const QString &text);
  void updateStatusBar();
  void onTasksLoaded(const QVector<ScheduledTask> &loaded);
  void onRequestFailed(const QString &message);
  void onTaskRunResult(bool ok, const QString &message);
  void onTaskSaved(bool ok, const QString &message, bool creating);

  int selectedRow() const;

  Ui::MainWindow *ui = nullptr;
  QLabel *statusLabel = nullptr;
  QLabel *serviceLabel = nullptr;

  QStandardItemModel *model = nullptr;
  QVector<ScheduledTask> tasks;

  ApiClient *api = nullptr;
  GossipDiscovery *discovery = nullptr;

  bool loading = false;
  bool opInProgress = false;
  QString opLabel;
  QString statusText;
  QString errorText;
  int spinnerIndex = 0;
  const QStringList spinnerFrames = {"-", "\\", "|", "/"};

  bool pendingRefresh = false;
  bool editorOpen = false;

  QTimer *spinnerTimer = nullptr;
  QTimer *autoRefreshTimer = nullptr;
  QTimer *statusClearTimer = nullptr;

  TaskEditorDialog *editorDialog = nullptr;
};
