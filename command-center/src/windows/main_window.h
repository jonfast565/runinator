#pragma once

#include <QLabel>
#include <QMainWindow>
#include <QGraphicsScene>
#include <QComboBox>
#include <QJsonObject>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSpinBox>
#include <QStandardItemModel>
#include <QTableView>
#include <QTimer>
#include <QStringList>

#include "clients/api_client.h"
#include "dialogs/task_editor_dialog.h"
#include "gossip/gossip_discovery.h"
#include "models/run_models.h"
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
  void setupGenericResourcePanel();
  void refreshTasks();
  void refreshRunsForSelectedTask();
  void refreshWorkflows();
  void refreshGenericRecords();
  void setupWorkflowDesigner();
  void updateTable();
  void updateRunsTable();
  void updateArtifactsTable();
  void updateWorkflowsTable();
  void updateWorkflowRunsTable();
  void updateGenericRecordsTable();
  void updateWorkflowDetails();
  void updateSelectedWorkflowNodeDetail();
  void populateStepEditor(const QString &nodeId);
  void applyStepEditor();
  void addWorkflow();
  void saveWorkflow();
  void addWorkflowStep();
  void removeWorkflowStep();
  WorkflowDefinition currentWorkflowDraft() const;
  void renderWorkflowGraph(const WorkflowDefinition &workflow);
  void updateWorkflowGraphGeometry();
  void updateRunNowState();
  void updateWorkflowActionState();
  void requestRunSelected();
  void requestWorkflowSelected();
  void approveSelectedApproval();
  void rejectSelectedApproval();
  void addNewTask();
  void editSelectedTask();
  void openEditor(const ScheduledTask &task, bool creating);
  void submitTask(const ScheduledTask &task, bool creating);
  void setStatus(const QString &text);
  void setError(const QString &text);
  void updateStatusBar();
  void onTasksLoaded(const QVector<ScheduledTask> &loaded);
  void onRunsLoaded(const QVector<RunSummary> &loaded);
  void onRunChunksLoaded(qint64 runId, const QVector<RunChunk> &loaded);
  void onRunArtifactsLoaded(qint64 runId, const QVector<RunArtifact> &loaded);
  void onWorkflowsLoaded(const QVector<WorkflowDefinition> &loaded);
  void onWorkflowSaved(const WorkflowDefinition &workflow);
  void onWorkflowRunRequested(qint64 workflowRunId);
  void onWorkflowRunsLoaded(qint64 workflowId, const QVector<WorkflowRunSummary> &runs);
  void onWorkflowRunLoaded(const WorkflowRunDetail &detail);
  void onGenericRecordsLoaded(const QString &endpoint, const QVector<QJsonObject> &records);
  void onApprovalActionFinished(bool ok, const QString &message);
  void onRequestFailed(const QString &message);
  void onTaskRunResult(bool ok, const QString &message);
  void onTaskSaved(bool ok, const QString &message, bool creating);

  int selectedRow() const;
  int selectedRunRow() const;
  int selectedWorkflowRow() const;
  int selectedWorkflowRunRow() const;
  int selectedGenericRecordRow() const;
  qint64 selectedGenericRecordId() const;
  QString selectedGenericEndpoint() const;
  QString genericRecordSummary(const QJsonObject &record) const;
  QString genericRecordType(const QJsonObject &record) const;

  Ui::MainWindow *ui = nullptr;
  QLabel *statusLabel = nullptr;
  QLabel *serviceLabel = nullptr;

  QStandardItemModel *model = nullptr;
  QStandardItemModel *runsModel = nullptr;
  QStandardItemModel *artifactsModel = nullptr;
  QStandardItemModel *workflowsModel = nullptr;
  QStandardItemModel *workflowRunsModel = nullptr;
  QStandardItemModel *genericRecordsModel = nullptr;
  QGraphicsScene *workflowScene = nullptr;
  QLineEdit *workflowNameEdit = nullptr;
  QSpinBox *workflowVersionSpin = nullptr;
  QSpinBox *workflowConcurrencySpin = nullptr;
  QPushButton *newWorkflowButton = nullptr;
  QPushButton *saveWorkflowButton = nullptr;
  QPushButton *addStepButton = nullptr;
  QPushButton *removeStepButton = nullptr;
  QPushButton *applyStepButton = nullptr;
  QLineEdit *nodeIdEdit = nullptr;
  QSpinBox *stepTaskIdSpin = nullptr;
  QLineEdit *stepNeedsEdit = nullptr;
  QSpinBox *stepRetrySpin = nullptr;
  QSpinBox *stepTimeoutSpin = nullptr;
  QPlainTextEdit *stepParametersEdit = nullptr;
  QPlainTextEdit *stepMappingsEdit = nullptr;
  QPlainTextEdit *workflowRunDetailEdit = nullptr;
  QTableView *workflowRunsTableView = nullptr;
  QComboBox *genericRecordTypeCombo = nullptr;
  QTableView *genericRecordsTableView = nullptr;
  QPlainTextEdit *genericRecordDetailEdit = nullptr;
  QPushButton *refreshGenericRecordsButton = nullptr;
  QPushButton *approveGenericApprovalButton = nullptr;
  QPushButton *rejectGenericApprovalButton = nullptr;
  QVector<ScheduledTask> tasks;
  QVector<RunSummary> runs;
  QVector<RunArtifact> artifacts;
  QVector<WorkflowDefinition> workflows;
  QVector<WorkflowRunSummary> workflowRuns;
  QVector<QJsonObject> genericRecords;
  WorkflowRunDetail currentWorkflowRun;
  qint64 selectedRunId = 0;
  qint64 selectedWorkflowRunId = 0;
  qint64 selectedWorkflowNodeTaskRunId = 0;
  std::optional<qint64> editingWorkflowId;
  QString selectedStepId;

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
