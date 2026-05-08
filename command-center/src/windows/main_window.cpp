#include "main_window.h"

#include "ui_main_window.h"

#include <QColor>
#include <QBrush>
#include <QGraphicsRectItem>
#include <QGraphicsTextItem>
#include <QFormLayout>
#include <QHeaderView>
#include <QHBoxLayout>
#include <QItemSelectionModel>
#include <QInputDialog>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeySequence>
#include <QMap>
#include <QPen>
#include <QShortcut>
#include <QSizePolicy>
#include <QVBoxLayout>

MainWindow::MainWindow(QWidget *parent) : QMainWindow(parent), ui(new Ui::MainWindow) {
  ui->setupUi(this);
  setWindowTitle("Command Center");

  api = new ApiClient(this);
  discovery = new GossipDiscovery(this);

  setupUiBindings();
  setupShortcuts();

  connect(api, &ApiClient::tasksLoaded, this, &MainWindow::onTasksLoaded);
  connect(api, &ApiClient::runsLoaded, this, &MainWindow::onRunsLoaded);
  connect(api, &ApiClient::runChunksLoaded, this, &MainWindow::onRunChunksLoaded);
  connect(api, &ApiClient::runArtifactsLoaded, this, &MainWindow::onRunArtifactsLoaded);
  connect(api, &ApiClient::workflowsLoaded, this, &MainWindow::onWorkflowsLoaded);
  connect(api, &ApiClient::workflowSaved, this, &MainWindow::onWorkflowSaved);
  connect(api, &ApiClient::workflowRunRequested, this, &MainWindow::onWorkflowRunRequested);
  connect(api, &ApiClient::workflowRunsLoaded, this, &MainWindow::onWorkflowRunsLoaded);
  connect(api, &ApiClient::workflowRunLoaded, this, &MainWindow::onWorkflowRunLoaded);
  connect(api, &ApiClient::requestFailed, this, &MainWindow::onRequestFailed);
  connect(api, &ApiClient::taskRunResult, this, &MainWindow::onTaskRunResult);
  connect(api, &ApiClient::taskSaved, this, &MainWindow::onTaskSaved);

  connect(discovery, &GossipDiscovery::serviceUrlChanged, this, [this](const QString &url) {
    api->setBaseUrl(url);
    serviceLabel->setText(QString("Service: %1").arg(url));
    if (pendingRefresh) {
      pendingRefresh = false;
      refreshTasks();
    }
  });
  connect(discovery, &GossipDiscovery::errorOccurred, this, &MainWindow::setError);
  discovery->start();

  spinnerTimer = new QTimer(this);
  spinnerTimer->setInterval(100);
  connect(spinnerTimer, &QTimer::timeout, this, [this]() {
    if (opInProgress || loading) {
      spinnerIndex = (spinnerIndex + 1) % spinnerFrames.size();
      updateStatusBar();
    }
  });
  spinnerTimer->start();

  autoRefreshTimer = new QTimer(this);
  autoRefreshTimer->setInterval(10000);
  connect(autoRefreshTimer, &QTimer::timeout, this, [this]() {
    if (!loading && !editorOpen) {
      refreshTasks();
      if (selectedWorkflowRunId > 0) {
        api->fetchWorkflowRun(selectedWorkflowRunId);
      }
    }
  });
  autoRefreshTimer->start();

  statusClearTimer = new QTimer(this);
  statusClearTimer->setInterval(5000);
  statusClearTimer->setSingleShot(true);
  connect(statusClearTimer, &QTimer::timeout, this, [this]() {
    if (!opInProgress && !loading) {
      statusText.clear();
      updateStatusBar();
    }
  });

  refreshTasks();
  refreshWorkflows();
}

MainWindow::~MainWindow() {
  delete editorDialog;
  delete ui;
}

void MainWindow::setupUiBindings() {
  model = new QStandardItemModel(this);
  model->setColumnCount(6);
  model->setHeaderData(0, Qt::Horizontal, "Name");
  model->setHeaderData(1, Qt::Horizontal, "Cron");
  model->setHeaderData(2, Qt::Horizontal, "Next Run");
  model->setHeaderData(3, Qt::Horizontal, "Enabled");
  model->setHeaderData(4, Qt::Horizontal, "Timeout");
  model->setHeaderData(5, Qt::Horizontal, "Action");

  ui->tableView->setModel(model);
  ui->tableView->setSelectionBehavior(QAbstractItemView::SelectRows);
  ui->tableView->setSelectionMode(QAbstractItemView::SingleSelection);
  ui->tableView->setEditTriggers(QAbstractItemView::NoEditTriggers);
  ui->tableView->horizontalHeader()->setStretchLastSection(true);
  ui->tableView->verticalHeader()->setVisible(false);

  runsModel = new QStandardItemModel(this);
  runsModel->setColumnCount(7);
  runsModel->setHeaderData(0, Qt::Horizontal, "Run");
  runsModel->setHeaderData(1, Qt::Horizontal, "Task");
  runsModel->setHeaderData(2, Qt::Horizontal, "Status");
  runsModel->setHeaderData(3, Qt::Horizontal, "Trigger");
  runsModel->setHeaderData(4, Qt::Horizontal, "Created");
  runsModel->setHeaderData(5, Qt::Horizontal, "Started");
  runsModel->setHeaderData(6, Qt::Horizontal, "Finished");
  ui->runsTableView->setModel(runsModel);
  ui->runsTableView->setSelectionBehavior(QAbstractItemView::SelectRows);
  ui->runsTableView->setSelectionMode(QAbstractItemView::SingleSelection);
  ui->runsTableView->setEditTriggers(QAbstractItemView::NoEditTriggers);
  ui->runsTableView->horizontalHeader()->setStretchLastSection(true);
  ui->runsTableView->verticalHeader()->setVisible(false);

  artifactsModel = new QStandardItemModel(this);
  artifactsModel->setColumnCount(5);
  artifactsModel->setHeaderData(0, Qt::Horizontal, "Name");
  artifactsModel->setHeaderData(1, Qt::Horizontal, "MIME");
  artifactsModel->setHeaderData(2, Qt::Horizontal, "Size");
  artifactsModel->setHeaderData(3, Qt::Horizontal, "URI");
  artifactsModel->setHeaderData(4, Qt::Horizontal, "Created");
  ui->artifactsTableView->setModel(artifactsModel);
  ui->artifactsTableView->setSelectionBehavior(QAbstractItemView::SelectRows);
  ui->artifactsTableView->setEditTriggers(QAbstractItemView::NoEditTriggers);
  ui->artifactsTableView->horizontalHeader()->setStretchLastSection(true);
  ui->artifactsTableView->verticalHeader()->setVisible(false);

  workflowsModel = new QStandardItemModel(this);
  workflowsModel->setColumnCount(4);
  workflowsModel->setHeaderData(0, Qt::Horizontal, "Name");
  workflowsModel->setHeaderData(1, Qt::Horizontal, "Version");
  workflowsModel->setHeaderData(2, Qt::Horizontal, "Enabled");
  workflowsModel->setHeaderData(3, Qt::Horizontal, "ID");
  ui->workflowsTableView->setModel(workflowsModel);
  ui->workflowsTableView->setSelectionBehavior(QAbstractItemView::SelectRows);
  ui->workflowsTableView->setSelectionMode(QAbstractItemView::SingleSelection);
  ui->workflowsTableView->setEditTriggers(QAbstractItemView::NoEditTriggers);
  ui->workflowsTableView->horizontalHeader()->setStretchLastSection(true);
  ui->workflowsTableView->verticalHeader()->setVisible(false);

  workflowRunsModel = new QStandardItemModel(this);
  workflowRunsModel->setColumnCount(5);
  workflowRunsModel->setHeaderData(0, Qt::Horizontal, "Run");
  workflowRunsModel->setHeaderData(1, Qt::Horizontal, "Status");
  workflowRunsModel->setHeaderData(2, Qt::Horizontal, "Created");
  workflowRunsModel->setHeaderData(3, Qt::Horizontal, "Started");
  workflowRunsModel->setHeaderData(4, Qt::Horizontal, "Finished");

  workflowScene = new QGraphicsScene(this);
  ui->workflowGraphView->setScene(workflowScene);
  setupWorkflowDesigner();

  connect(ui->tableView, &QTableView::doubleClicked, this, [this]() { editSelectedTask(); });

  ui->actionRefresh->setShortcut(QKeySequence(Qt::CTRL | Qt::Key_R));
  ui->actionAdd->setShortcut(QKeySequence(Qt::CTRL | Qt::Key_N));
  ui->actionEdit->setShortcut(QKeySequence(Qt::Key_E));
  ui->actionQuit->setShortcut(QKeySequence(Qt::Key_Q));
  ui->actionQuit->setShortcuts({QKeySequence(Qt::Key_Q), QKeySequence(Qt::Key_Escape)});

  connect(ui->actionRefresh, &QAction::triggered, this, &MainWindow::refreshTasks);
  connect(ui->actionRunNow, &QAction::triggered, this, &MainWindow::requestRunSelected);
  connect(ui->actionRunWorkflow, &QAction::triggered, this, &MainWindow::requestWorkflowSelected);
  connect(ui->actionEdit, &QAction::triggered, this, &MainWindow::editSelectedTask);
  connect(ui->actionAdd, &QAction::triggered, this, &MainWindow::addNewTask);
  connect(ui->actionQuit, &QAction::triggered, this, &QWidget::close);

  statusLabel = new QLabel("Ready.", this);
  statusLabel->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
  serviceLabel = new QLabel("No service discovered", this);

  statusBar()->addWidget(statusLabel, 1);
  statusBar()->addPermanentWidget(serviceLabel);

  connect(ui->tableView->selectionModel(), &QItemSelectionModel::selectionChanged, this,
          [this]() {
            updateRunNowState();
            refreshRunsForSelectedTask();
          });
  connect(ui->runsTableView->selectionModel(), &QItemSelectionModel::selectionChanged, this,
          [this]() {
            const int row = selectedRunRow();
            if (row < 0 || row >= runs.size()) {
              selectedRunId = 0;
              ui->runOutputEdit->clear();
              artifacts.clear();
              updateArtifactsTable();
              return;
            }
            selectedRunId = runs[row].id;
            api->fetchRunChunks(selectedRunId);
            api->fetchRunArtifacts(selectedRunId);
          });
  connect(ui->workflowsTableView->selectionModel(), &QItemSelectionModel::selectionChanged, this,
          [this]() {
            updateWorkflowActionState();
            updateWorkflowDetails();
          });
  connect(workflowScene, &QGraphicsScene::selectionChanged, this, [this]() {
    const auto selected = workflowScene->selectedItems();
    if (selected.isEmpty()) {
      return;
    }
    const QString stepId = selected.first()->data(0).toString();
    if (!stepId.isEmpty()) {
      populateStepEditor(stepId);
      updateSelectedWorkflowNodeDetail();
    }
  });
  connect(ui->tabWidget, &QTabWidget::currentChanged, this, [this](int index) {
    if (ui->tabWidget->widget(index) == ui->workflowsTab) {
      refreshWorkflows();
    } else if (ui->tabWidget->widget(index) == ui->runsTab) {
      refreshRunsForSelectedTask();
    }
  });

  updateRunNowState();
  updateWorkflowActionState();
}

void MainWindow::setupWorkflowDesigner() {
  auto *toolbarLayout = new QHBoxLayout();
  newWorkflowButton = new QPushButton("New", this);
  saveWorkflowButton = new QPushButton("Save", this);
  addStepButton = new QPushButton("Add Step", this);
  removeStepButton = new QPushButton("Remove Step", this);
  toolbarLayout->addWidget(new QLabel("Name:", this));
  workflowNameEdit = new QLineEdit(this);
  toolbarLayout->addWidget(workflowNameEdit, 2);
  toolbarLayout->addWidget(new QLabel("Version:", this));
  workflowVersionSpin = new QSpinBox(this);
  workflowVersionSpin->setMinimum(1);
  workflowVersionSpin->setMaximum(999999);
  toolbarLayout->addWidget(workflowVersionSpin);
  toolbarLayout->addWidget(new QLabel("Concurrency:", this));
  workflowConcurrencySpin = new QSpinBox(this);
  workflowConcurrencySpin->setMinimum(1);
  workflowConcurrencySpin->setMaximum(256);
  toolbarLayout->addWidget(workflowConcurrencySpin);
  toolbarLayout->addWidget(newWorkflowButton);
  toolbarLayout->addWidget(saveWorkflowButton);
  toolbarLayout->addWidget(addStepButton);
  toolbarLayout->addWidget(removeStepButton);
  ui->workflowsLayout->insertLayout(0, toolbarLayout);

  auto *form = new QFormLayout();
  stepIdEdit = new QLineEdit(this);
  stepTaskIdSpin = new QSpinBox(this);
  stepTaskIdSpin->setMinimum(1);
  stepTaskIdSpin->setMaximum(2147483647);
  stepNeedsEdit = new QLineEdit(this);
  stepRetrySpin = new QSpinBox(this);
  stepRetrySpin->setMinimum(1);
  stepRetrySpin->setMaximum(100);
  stepTimeoutSpin = new QSpinBox(this);
  stepTimeoutSpin->setMinimum(0);
  stepTimeoutSpin->setMaximum(2147483647);
  stepParametersEdit = new QPlainTextEdit(this);
  stepParametersEdit->setMaximumHeight(90);
  stepMappingsEdit = new QPlainTextEdit(this);
  stepMappingsEdit->setMaximumHeight(90);
  workflowRunDetailEdit = new QPlainTextEdit(this);
  workflowRunDetailEdit->setReadOnly(true);
  workflowRunDetailEdit->setMaximumHeight(180);
  applyStepButton = new QPushButton("Apply Step", this);
  workflowRunsTableView = new QTableView(this);
  workflowRunsTableView->setModel(workflowRunsModel);
  workflowRunsTableView->setSelectionBehavior(QAbstractItemView::SelectRows);
  workflowRunsTableView->setSelectionMode(QAbstractItemView::SingleSelection);
  workflowRunsTableView->setEditTriggers(QAbstractItemView::NoEditTriggers);
  workflowRunsTableView->horizontalHeader()->setStretchLastSection(true);
  workflowRunsTableView->verticalHeader()->setVisible(false);
  workflowRunsTableView->setMaximumHeight(140);
  form->addRow("Step ID", stepIdEdit);
  form->addRow("Task ID", stepTaskIdSpin);
  form->addRow("Needs", stepNeedsEdit);
  form->addRow("Max Attempts", stepRetrySpin);
  form->addRow("Timeout Seconds", stepTimeoutSpin);
  form->addRow("Parameters JSON", stepParametersEdit);
  form->addRow("Mappings JSON", stepMappingsEdit);
  form->addRow(applyStepButton);
  form->addRow("Run History", workflowRunsTableView);
  form->addRow("Run Detail", workflowRunDetailEdit);
  ui->workflowDetailsSplitter->addWidget(new QWidget(this));
  ui->workflowDetailsSplitter->widget(2)->setLayout(form);

  connect(newWorkflowButton, &QPushButton::clicked, this, &MainWindow::addWorkflow);
  connect(saveWorkflowButton, &QPushButton::clicked, this, &MainWindow::saveWorkflow);
  connect(addStepButton, &QPushButton::clicked, this, &MainWindow::addWorkflowStep);
  connect(removeStepButton, &QPushButton::clicked, this, &MainWindow::removeWorkflowStep);
  connect(applyStepButton, &QPushButton::clicked, this, &MainWindow::applyStepEditor);
  connect(workflowRunsTableView->selectionModel(), &QItemSelectionModel::selectionChanged, this, [this]() {
    const int row = selectedWorkflowRunRow();
    if (row < 0 || row >= workflowRuns.size()) {
      selectedWorkflowRunId = 0;
      return;
    }
    selectedWorkflowRunId = workflowRuns[row].id;
    api->fetchWorkflowRun(selectedWorkflowRunId);
  });
}

void MainWindow::setupShortcuts() {
  auto *refreshShortcut = new QShortcut(QKeySequence(Qt::Key_R), this);
  connect(refreshShortcut, &QShortcut::activated, this, &MainWindow::refreshTasks);

  auto *runShortcut = new QShortcut(QKeySequence(Qt::Key_Return), this);
  connect(runShortcut, &QShortcut::activated, this, &MainWindow::requestRunSelected);

  auto *runShortcutAlt = new QShortcut(QKeySequence(Qt::Key_Enter), this);
  connect(runShortcutAlt, &QShortcut::activated, this, &MainWindow::requestRunSelected);
}

void MainWindow::refreshTasks() {
  if (loading) {
    return;
  }
  if (api->baseUrl().isEmpty()) {
    pendingRefresh = true;
    setStatus("Waiting for service discovery...");
    return;
  }

  loading = true;
  opInProgress = true;
  opLabel = "Refreshing tasks";
  updateStatusBar();

  api->fetchTasks();
}

void MainWindow::refreshRunsForSelectedTask() {
  const int row = selectedRow();
  if (row < 0 || row >= tasks.size() || !tasks[row].id.has_value()) {
    runs.clear();
    artifacts.clear();
    selectedRunId = 0;
    selectedWorkflowNodeTaskRunId = 0;
    updateRunsTable();
    updateArtifactsTable();
    ui->runOutputEdit->clear();
    return;
  }
  api->fetchRuns(tasks[row].id.value());
}

void MainWindow::refreshWorkflows() {
  if (api->baseUrl().isEmpty()) {
    return;
  }
  api->fetchWorkflows();
}

void MainWindow::onTasksLoaded(const QVector<ScheduledTask> &loaded) {
  loading = false;
  opInProgress = false;
  opLabel.clear();

  tasks = loaded;
  updateTable();
  refreshRunsForSelectedTask();
  setStatus("Refreshed.");
}

void MainWindow::onRunsLoaded(const QVector<RunSummary> &loaded) {
  runs = loaded;
  updateRunsTable();
}

void MainWindow::onRunChunksLoaded(qint64 runId, const QVector<RunChunk> &loaded) {
  if (runId == selectedWorkflowNodeTaskRunId) {
    QStringList lines = workflowRunDetailEdit->toPlainText().split('\n');
    lines.push_back("");
    lines.push_back(QString("Task run %1 chunks").arg(runId));
    for (const RunChunk &chunk : loaded) {
      lines.push_back(QString("[%1] %2").arg(chunk.stream, chunk.content));
    }
    workflowRunDetailEdit->setPlainText(lines.join('\n'));
    return;
  }
  if (runId != selectedRunId) {
    return;
  }
  QStringList lines;
  for (const RunChunk &chunk : loaded) {
    lines.push_back(QString("[%1] %2").arg(chunk.stream, chunk.content));
  }
  ui->runOutputEdit->setPlainText(lines.join('\n'));
}

void MainWindow::onRunArtifactsLoaded(qint64 runId, const QVector<RunArtifact> &loaded) {
  if (runId == selectedWorkflowNodeTaskRunId) {
    QStringList lines = workflowRunDetailEdit->toPlainText().split('\n');
    lines.push_back("");
    lines.push_back(QString("Task run %1 artifacts").arg(runId));
    for (const RunArtifact &artifact : loaded) {
      lines.push_back(QString("%1 (%2 bytes) %3").arg(artifact.name).arg(artifact.sizeBytes).arg(artifact.uri));
    }
    workflowRunDetailEdit->setPlainText(lines.join('\n'));
    return;
  }
  if (runId != selectedRunId) {
    return;
  }
  artifacts = loaded;
  updateArtifactsTable();
}

void MainWindow::onWorkflowsLoaded(const QVector<WorkflowDefinition> &loaded) {
  workflows = loaded;
  updateWorkflowsTable();
  updateWorkflowDetails();
}

void MainWindow::onWorkflowSaved(const WorkflowDefinition &workflow) {
  editingWorkflowId = workflow.id;
  setStatus(QString("Workflow saved: %1").arg(workflow.name));
  refreshWorkflows();
}

void MainWindow::onWorkflowRunRequested(qint64 workflowRunId) {
  selectedWorkflowRunId = workflowRunId;
  setStatus(QString("Workflow run queued: %1").arg(workflowRunId));
  api->fetchWorkflowRun(workflowRunId);
  const int row = selectedWorkflowRow();
  if (row >= 0 && row < workflows.size() && workflows[row].id.has_value()) {
    api->fetchWorkflowRuns(workflows[row].id.value());
  }
}

void MainWindow::onWorkflowRunsLoaded(qint64 workflowId, const QVector<WorkflowRunSummary> &loaded) {
  const int row = selectedWorkflowRow();
  if (row >= 0 && row < workflows.size() && workflows[row].id.has_value() &&
      workflows[row].id.value() != workflowId) {
    return;
  }
  workflowRuns = loaded;
  updateWorkflowRunsTable();
}

void MainWindow::onWorkflowRunLoaded(const WorkflowRunDetail &detail) {
  if (selectedWorkflowRunId > 0 && detail.id != selectedWorkflowRunId) {
    return;
  }
  currentWorkflowRun = detail;
  QStringList lines;
  lines.push_back(QString("Run %1: %2").arg(detail.id).arg(detail.status));
  lines.push_back(QString("Started: %1").arg(formatOptionalDateTime(detail.startedAt)));
  lines.push_back(QString("Finished: %1").arg(formatOptionalDateTime(detail.finishedAt)));
  if (!detail.message.isEmpty()) {
    lines.push_back(QString("Message: %1").arg(detail.message));
  }
  for (const WorkflowStepRun &step : detail.steps) {
    lines.push_back(QString("%1: %2, attempt %3, task run %4%5")
                        .arg(step.stepId)
                        .arg(step.status)
                        .arg(step.attempt)
                        .arg(step.taskRunId.has_value() ? QString::number(step.taskRunId.value()) : "-")
                        .arg(step.message.isEmpty() ? QString() : QString(", %1").arg(step.message)));
  }
  workflowRunDetailEdit->setPlainText(lines.join('\n'));
  updateSelectedWorkflowNodeDetail();

  const int row = selectedWorkflowRow();
  if (row >= 0 && row < workflows.size()) {
    renderWorkflowGraph(workflows[row]);
  }
}

void MainWindow::onRequestFailed(const QString &message) {
  loading = false;
  opInProgress = false;
  opLabel.clear();
  setError(message);
}

void MainWindow::onTaskRunResult(bool ok, const QString &message) {
  opInProgress = false;
  opLabel.clear();

  setStatus(QString("%1: %2").arg(ok ? "OK" : "ERR").arg(message));
  refreshTasks();
  refreshRunsForSelectedTask();
}

void MainWindow::onTaskSaved(bool ok, const QString &message, bool creating) {
  opInProgress = false;
  opLabel.clear();

  if (!ok) {
    setError(message);
    if (editorDialog) {
      editorDialog->setError(message);
      editorDialog->setSaving(false);
    }
    return;
  }

  setStatus(QString("OK: %1").arg(message));
  if (editorDialog) {
    editorDialog->accept();
  }
  refreshTasks();
}

void MainWindow::updateTable() {
  const int previousSelection = selectedRow();
  model->removeRows(0, model->rowCount());

  for (int i = 0; i < tasks.size(); ++i) {
    const ScheduledTask &task = tasks[i];
    QList<QStandardItem *> row;
    row.append(new QStandardItem(task.name));
    row.append(new QStandardItem(task.cronSchedule));
    row.append(new QStandardItem(formatDate(task.nextExecution)));
    row.append(new QStandardItem(task.enabled ? "Yes" : "No"));
    row.append(new QStandardItem(QString("%1 ms").arg(task.timeout)));
    row.append(new QStandardItem("Edit"));

    if (!task.enabled) {
      for (auto *item : row) {
        item->setForeground(QColor("#7f8c8d"));
      }
    }
    for (auto *item : row) {
      item->setEditable(false);
    }
    model->appendRow(row);
  }

  if (!tasks.isEmpty()) {
    int newSelection = previousSelection;
    if (newSelection < 0 || newSelection >= tasks.size()) {
      newSelection = 0;
    }
    ui->tableView->selectRow(newSelection);
  }

  updateRunNowState();
}

void MainWindow::updateRunsTable() {
  const int previousSelection = selectedRunRow();
  runsModel->removeRows(0, runsModel->rowCount());

  for (const RunSummary &run : runs) {
    QList<QStandardItem *> row;
    row.append(new QStandardItem(QString::number(run.id)));
    row.append(new QStandardItem(QString::number(run.taskId)));
    row.append(new QStandardItem(run.status));
    row.append(new QStandardItem(run.trigger));
    row.append(new QStandardItem(formatDateTime(run.createdAt)));
    row.append(new QStandardItem(formatOptionalDateTime(run.startedAt)));
    row.append(new QStandardItem(formatOptionalDateTime(run.finishedAt)));
    for (auto *item : row) {
      item->setEditable(false);
      if (run.status == "failed" || run.status == "timed_out") {
        item->setForeground(QColor("#c0392b"));
      } else if (run.status == "succeeded") {
        item->setForeground(QColor("#1f7a4d"));
      }
    }
    runsModel->appendRow(row);
  }

  if (!runs.isEmpty()) {
    int newSelection = previousSelection;
    if (newSelection < 0 || newSelection >= runs.size()) {
      newSelection = 0;
    }
    ui->runsTableView->selectRow(newSelection);
  }
}

void MainWindow::updateArtifactsTable() {
  artifactsModel->removeRows(0, artifactsModel->rowCount());
  for (const RunArtifact &artifact : artifacts) {
    QList<QStandardItem *> row;
    row.append(new QStandardItem(artifact.name));
    row.append(new QStandardItem(artifact.mimeType));
    row.append(new QStandardItem(QString::number(artifact.sizeBytes)));
    row.append(new QStandardItem(artifact.uri));
    row.append(new QStandardItem(formatDateTime(artifact.createdAt)));
    for (auto *item : row) {
      item->setEditable(false);
    }
    artifactsModel->appendRow(row);
  }
}

void MainWindow::updateWorkflowsTable() {
  const int previousSelection = selectedWorkflowRow();
  workflowsModel->removeRows(0, workflowsModel->rowCount());

  for (const WorkflowDefinition &workflow : workflows) {
    QList<QStandardItem *> row;
    row.append(new QStandardItem(workflow.name));
    row.append(new QStandardItem(QString::number(workflow.version)));
    row.append(new QStandardItem(workflow.enabled ? "Yes" : "No"));
    row.append(new QStandardItem(workflow.id.has_value() ? QString::number(workflow.id.value()) : "-"));
    for (auto *item : row) {
      item->setEditable(false);
      if (!workflow.enabled) {
        item->setForeground(QColor("#7f8c8d"));
      }
    }
    workflowsModel->appendRow(row);
  }

  if (!workflows.isEmpty()) {
    int newSelection = previousSelection;
    if (newSelection < 0 || newSelection >= workflows.size()) {
      newSelection = 0;
    }
    ui->workflowsTableView->selectRow(newSelection);
  }
  updateWorkflowActionState();
}

void MainWindow::updateWorkflowRunsTable() {
  const int previousSelection = selectedWorkflowRunRow();
  workflowRunsModel->removeRows(0, workflowRunsModel->rowCount());

  for (const WorkflowRunSummary &run : workflowRuns) {
    QList<QStandardItem *> row;
    row.append(new QStandardItem(QString::number(run.id)));
    row.append(new QStandardItem(run.status));
    row.append(new QStandardItem(formatDateTime(run.createdAt)));
    row.append(new QStandardItem(formatOptionalDateTime(run.startedAt)));
    row.append(new QStandardItem(formatOptionalDateTime(run.finishedAt)));
    for (auto *item : row) {
      item->setEditable(false);
      if (run.status == "failed" || run.status == "timed_out") {
        item->setForeground(QColor("#c0392b"));
      } else if (run.status == "succeeded") {
        item->setForeground(QColor("#1f7a4d"));
      }
    }
    workflowRunsModel->appendRow(row);
  }

  if (!workflowRuns.isEmpty()) {
    int newSelection = previousSelection;
    if (selectedWorkflowRunId > 0) {
      for (int i = 0; i < workflowRuns.size(); ++i) {
        if (workflowRuns[i].id == selectedWorkflowRunId) {
          newSelection = i;
          break;
        }
      }
    }
    if (newSelection < 0 || newSelection >= workflowRuns.size()) {
      newSelection = 0;
    }
    workflowRunsTableView->selectRow(newSelection);
    selectedWorkflowRunId = workflowRuns[newSelection].id;
  }
}

void MainWindow::updateWorkflowDetails() {
  const int row = selectedWorkflowRow();
  if (row < 0 || row >= workflows.size()) {
    ui->workflowDefinitionEdit->clear();
    workflowScene->clear();
    workflowRuns.clear();
    workflowRunsModel->removeRows(0, workflowRunsModel->rowCount());
    workflowRunDetailEdit->clear();
    editingWorkflowId.reset();
    return;
  }

  const WorkflowDefinition &workflow = workflows[row];
  editingWorkflowId = workflow.id;
  workflowNameEdit->setText(workflow.name);
  workflowVersionSpin->setValue(static_cast<int>(workflow.version));
  workflowConcurrencySpin->setValue(
      static_cast<int>(workflow.definition.value("concurrency").toInt(1)));
  ui->workflowDefinitionEdit->setPlainText(
      QJsonDocument(workflow.definition).toJson(QJsonDocument::Indented));
  renderWorkflowGraph(workflow);
  if (workflow.id.has_value()) {
    api->fetchWorkflowRuns(workflow.id.value());
  }
}

void MainWindow::populateStepEditor(const QString &stepId) {
  selectedStepId = stepId;
  QJsonObject definition = currentWorkflowDraft().definition;
  const QJsonArray steps = definition.value("steps").toArray();
  for (const auto &value : steps) {
    const QJsonObject step = value.toObject();
    if (step.value("id").toString() != stepId) {
      continue;
    }
    stepIdEdit->setText(stepId);
    stepTaskIdSpin->setValue(step.value("task_id").toInt(1));
    QStringList needs;
    for (const auto &need : step.value("needs").toArray()) {
      needs.push_back(need.toString());
    }
    stepNeedsEdit->setText(needs.join(","));
    stepRetrySpin->setValue(step.value("retry").toObject().value("max_attempts").toInt(1));
    stepTimeoutSpin->setValue(step.value("timeout_seconds").toInt(0));
    stepParametersEdit->setPlainText(QJsonDocument(step.value("parameters").toObject())
                                         .toJson(QJsonDocument::Indented));
    stepMappingsEdit->setPlainText(QJsonDocument(step.value("mappings").toArray())
                                       .toJson(QJsonDocument::Indented));
    updateSelectedWorkflowNodeDetail();
    return;
  }
}

void MainWindow::updateSelectedWorkflowNodeDetail() {
  if (selectedStepId.isEmpty() || currentWorkflowRun.id == 0) {
    return;
  }
  selectedWorkflowNodeTaskRunId = 0;
  for (const WorkflowStepRun &step : currentWorkflowRun.steps) {
    if (step.stepId == selectedStepId && step.taskRunId.has_value()) {
      selectedWorkflowNodeTaskRunId = step.taskRunId.value();
      api->fetchRunChunks(selectedWorkflowNodeTaskRunId);
      api->fetchRunArtifacts(selectedWorkflowNodeTaskRunId);
      return;
    }
  }
}

void MainWindow::applyStepEditor() {
  WorkflowDefinition workflow = currentWorkflowDraft();
  QJsonObject definition = workflow.definition;
  QJsonArray steps = definition.value("steps").toArray();
  for (int i = 0; i < steps.size(); ++i) {
    QJsonObject step = steps[i].toObject();
    if (step.value("id").toString() != selectedStepId) {
      continue;
    }
    step.insert("id", stepIdEdit->text().trimmed());
    step.insert("task_id", stepTaskIdSpin->value());
    QJsonArray needs;
    for (const QString &need : stepNeedsEdit->text().split(',', Qt::SkipEmptyParts)) {
      needs.append(need.trimmed());
    }
    step.insert("needs", needs);
    step.insert("retry", QJsonObject{{"max_attempts", stepRetrySpin->value()}});
    if (stepTimeoutSpin->value() > 0) {
      step.insert("timeout_seconds", stepTimeoutSpin->value());
    } else {
      step.remove("timeout_seconds");
    }

    QJsonParseError paramsError;
    const QJsonDocument paramsDoc =
        QJsonDocument::fromJson(stepParametersEdit->toPlainText().toUtf8(), &paramsError);
    if (paramsError.error != QJsonParseError::NoError || !paramsDoc.isObject()) {
      setError("Step parameters must be a JSON object");
      return;
    }
    step.insert("parameters", paramsDoc.object());

    QJsonParseError mappingsError;
    const QJsonDocument mappingsDoc =
        QJsonDocument::fromJson(stepMappingsEdit->toPlainText().toUtf8(), &mappingsError);
    if (mappingsError.error != QJsonParseError::NoError || !mappingsDoc.isArray()) {
      setError("Step mappings must be a JSON array");
      return;
    }
    step.insert("mappings", mappingsDoc.array());

    steps[i] = step;
    selectedStepId = stepIdEdit->text().trimmed();
    break;
  }
  definition.insert("steps", steps);
  workflow.definition = definition;
  ui->workflowDefinitionEdit->setPlainText(QJsonDocument(definition).toJson(QJsonDocument::Indented));
  renderWorkflowGraph(workflow);
}

void MainWindow::addWorkflow() {
  WorkflowDefinition workflow;
  workflow.name = "New Workflow";
  workflow.version = 1;
  workflow.enabled = true;
  workflow.inputSchema = QJsonObject{{"type", "object"}, {"additionalProperties", true}};
  workflow.definition = QJsonObject{{"concurrency", 1}, {"steps", QJsonArray()}};
  workflows.push_back(workflow);
  updateWorkflowsTable();
  ui->workflowsTableView->selectRow(workflows.size() - 1);
}

void MainWindow::saveWorkflow() {
  applyStepEditor();
  api->saveWorkflow(currentWorkflowDraft());
}

void MainWindow::addWorkflowStep() {
  WorkflowDefinition workflow = currentWorkflowDraft();
  QJsonObject definition = workflow.definition;
  QJsonArray steps = definition.value("steps").toArray();
  const QString id = QString("step_%1").arg(steps.size() + 1);
  steps.append(QJsonObject{
      {"id", id},
      {"task_id", tasks.isEmpty() || !tasks.first().id.has_value() ? 1 : static_cast<int>(tasks.first().id.value())},
      {"needs", QJsonArray()},
      {"parameters", QJsonObject()},
      {"retry", QJsonObject{{"max_attempts", 1}}},
      {"mappings", QJsonArray()},
  });
  definition.insert("steps", steps);
  workflow.definition = definition;
  ui->workflowDefinitionEdit->setPlainText(QJsonDocument(definition).toJson(QJsonDocument::Indented));
  renderWorkflowGraph(workflow);
  populateStepEditor(id);
}

void MainWindow::removeWorkflowStep() {
  if (selectedStepId.isEmpty()) {
    return;
  }
  WorkflowDefinition workflow = currentWorkflowDraft();
  QJsonObject definition = workflow.definition;
  QJsonArray next;
  for (const auto &value : definition.value("steps").toArray()) {
    QJsonObject step = value.toObject();
    if (step.value("id").toString() == selectedStepId) {
      continue;
    }
    QJsonArray needs;
    for (const auto &need : step.value("needs").toArray()) {
      if (need.toString() != selectedStepId) {
        needs.append(need);
      }
    }
    step.insert("needs", needs);
    next.append(step);
  }
  selectedStepId.clear();
  definition.insert("steps", next);
  workflow.definition = definition;
  ui->workflowDefinitionEdit->setPlainText(QJsonDocument(definition).toJson(QJsonDocument::Indented));
  renderWorkflowGraph(workflow);
}

WorkflowDefinition MainWindow::currentWorkflowDraft() const {
  WorkflowDefinition workflow;
  workflow.id = editingWorkflowId;
  workflow.name = workflowNameEdit->text().trimmed().isEmpty() ? "Untitled Workflow" : workflowNameEdit->text().trimmed();
  workflow.version = workflowVersionSpin->value();
  workflow.enabled = true;
  workflow.inputSchema = QJsonObject{{"type", "object"}, {"additionalProperties", true}};
  QJsonParseError parseError;
  QJsonDocument doc = QJsonDocument::fromJson(ui->workflowDefinitionEdit->toPlainText().toUtf8(), &parseError);
  workflow.definition = parseError.error == QJsonParseError::NoError && doc.isObject()
                            ? doc.object()
                            : QJsonObject{{"steps", QJsonArray()}};
  workflow.definition.insert("concurrency", workflowConcurrencySpin->value());
  QJsonObject nodes;
  for (QGraphicsItem *item : workflowScene->items()) {
    auto *rect = qgraphicsitem_cast<QGraphicsRectItem *>(item);
    if (!rect) {
      continue;
    }
    const QString stepId = rect->data(0).toString();
    if (stepId.isEmpty()) {
      continue;
    }
    nodes.insert(stepId, QJsonObject{{"x", rect->pos().x()}, {"y", rect->pos().y()}});
  }
  QJsonObject layout;
  layout.insert("nodes", nodes);
  QJsonObject ui;
  ui.insert("layout", layout);
  workflow.definition.insert("ui", ui);
  return workflow;
}

void MainWindow::renderWorkflowGraph(const WorkflowDefinition &workflow) {
  workflowScene->clear();
  const QJsonArray steps = workflow.definition.value("steps").toArray();
  const QJsonObject nodeLayout = workflow.definition.value("ui").toObject()
                                     .value("layout").toObject()
                                     .value("nodes").toObject();
  QMap<QString, QPointF> positions;
  const int nodeWidth = 150;
  const int nodeHeight = 54;
  const int xGap = 220;
  const int yGap = 90;

  for (int i = 0; i < steps.size(); ++i) {
    const QJsonObject step = steps[i].toObject();
    const QString id = step.value("id").toString(QString("step_%1").arg(i + 1));
    const QJsonObject layout = nodeLayout.value(id).toObject();
    if (layout.contains("x") && layout.contains("y")) {
      positions.insert(id, QPointF(layout.value("x").toDouble(), layout.value("y").toDouble()));
    } else {
      positions.insert(id, QPointF((i % 4) * xGap, (i / 4) * yGap));
    }
  }

  QPen edgePen(QColor("#7f8c8d"));
  edgePen.setWidth(2);
  for (const auto &value : steps) {
    const QJsonObject step = value.toObject();
    const QString id = step.value("id").toString();
    const QPointF to = positions.value(id) + QPointF(0, nodeHeight / 2);
    for (const auto &depValue : step.value("needs").toArray()) {
      const QString dep = depValue.toString();
      if (!positions.contains(dep)) {
        continue;
      }
      const QPointF from = positions.value(dep) + QPointF(nodeWidth, nodeHeight / 2);
      workflowScene->addLine(QLineF(from, to), edgePen);
    }
  }

  QMap<QString, WorkflowStepRun> stepRunById;
  if (selectedWorkflowRunId > 0 && currentWorkflowRun.id == selectedWorkflowRunId &&
      workflow.id.has_value() && currentWorkflowRun.workflowId == workflow.id.value()) {
    for (const WorkflowStepRun &stepRun : currentWorkflowRun.steps) {
      stepRunById.insert(stepRun.stepId, stepRun);
    }
  }

  QPen nodePen(QColor("#34495e"));
  for (const auto &value : steps) {
    const QJsonObject step = value.toObject();
    const QString id = step.value("id").toString();
    const QPointF pos = positions.value(id);
    QColor fill("#f8fafc");
    QString statusLine;
    if (stepRunById.contains(id)) {
      const WorkflowStepRun stepRun = stepRunById.value(id);
      statusLine = QString("\n%1 a%2").arg(stepRun.status).arg(stepRun.attempt);
      if (stepRun.status == "succeeded") {
        fill = QColor("#d5f5e3");
      } else if (stepRun.status == "failed" || stepRun.status == "timed_out" ||
                 stepRun.status == "canceled") {
        fill = QColor("#fadbd8");
      } else if (stepRun.status == "running") {
        fill = QColor("#fcf3cf");
      } else {
        fill = QColor("#eaf2f8");
      }
    }
    QGraphicsRectItem *node =
        workflowScene->addRect(QRectF(QPointF(0, 0), QSizeF(nodeWidth, nodeHeight)), nodePen, QBrush(fill));
    node->setPos(pos);
    node->setData(0, id);
    node->setFlags(QGraphicsItem::ItemIsMovable | QGraphicsItem::ItemIsSelectable);
    QGraphicsTextItem *label = workflowScene->addText(
        QString("%1\nTask %2%3").arg(id).arg(step.value("task_id").toVariant().toLongLong()).arg(statusLine));
    label->setDefaultTextColor(QColor("#2c3e50"));
    label->setPos(pos + QPointF(8, 6));
    label->setData(0, id);
  }
  ui->workflowGraphView->fitInView(workflowScene->itemsBoundingRect().adjusted(-24, -24, 24, 24),
                                  Qt::KeepAspectRatio);
}

int MainWindow::selectedRow() const {
  const QModelIndexList selection = ui->tableView->selectionModel()->selectedRows();
  if (selection.isEmpty()) {
    return -1;
  }
  return selection.first().row();
}

int MainWindow::selectedRunRow() const {
  const QModelIndexList selection = ui->runsTableView->selectionModel()->selectedRows();
  if (selection.isEmpty()) {
    return -1;
  }
  return selection.first().row();
}

int MainWindow::selectedWorkflowRow() const {
  const QModelIndexList selection = ui->workflowsTableView->selectionModel()->selectedRows();
  if (selection.isEmpty()) {
    return -1;
  }
  return selection.first().row();
}

int MainWindow::selectedWorkflowRunRow() const {
  if (!workflowRunsTableView || !workflowRunsTableView->selectionModel()) {
    return -1;
  }
  const QModelIndexList selection = workflowRunsTableView->selectionModel()->selectedRows();
  if (!selection.isEmpty()) {
    return selection.first().row();
  }
  return -1;
}

void MainWindow::updateRunNowState() {
  int row = selectedRow();
  bool enabled = false;
  if (row >= 0 && row < tasks.size()) {
    enabled = tasks[row].enabled;
  }
  ui->actionRunNow->setEnabled(enabled);
  ui->actionEdit->setEnabled(row >= 0);
}

void MainWindow::updateWorkflowActionState() {
  const int row = selectedWorkflowRow();
  ui->actionRunWorkflow->setEnabled(row >= 0 && row < workflows.size() && workflows[row].enabled &&
                                    workflows[row].id.has_value());
}

void MainWindow::requestRunSelected() {
  const int row = selectedRow();
  if (row < 0 || row >= tasks.size()) {
    setError("No task selected");
    return;
  }
  const ScheduledTask &task = tasks[row];
  if (!task.enabled) {
    setError("Task is disabled");
    return;
  }
  if (!task.id.has_value()) {
    setError("Task is missing an ID");
    return;
  }

  opInProgress = true;
  opLabel = QString("Running %1").arg(task.name);
  updateStatusBar();

  api->requestRun(task.id.value());
}

void MainWindow::requestWorkflowSelected() {
  const int row = selectedWorkflowRow();
  if (row < 0 || row >= workflows.size()) {
    setError("No workflow selected");
    return;
  }
  const WorkflowDefinition &workflow = workflows[row];
  if (!workflow.enabled) {
    setError("Workflow is disabled");
    return;
  }
  if (!workflow.id.has_value()) {
    setError("Workflow is missing an ID");
    return;
  }
  opInProgress = true;
  opLabel = QString("Running workflow %1").arg(workflow.name);
  updateStatusBar();
  api->createWorkflowRun(workflow.id.value());
}

void MainWindow::addNewTask() {
  ScheduledTask task;
  task.enabled = true;
  task.timeout = 0;
  task.inputSchema.insert("type", "object");
  task.inputSchema.insert("additionalProperties", true);
  openEditor(task, true);
}

void MainWindow::editSelectedTask() {
  const int row = selectedRow();
  if (row < 0 || row >= tasks.size()) {
    setError("No task selected");
    return;
  }
  openEditor(tasks[row], false);
}

void MainWindow::openEditor(const ScheduledTask &task, bool creating) {
  if (editorOpen) {
    return;
  }
  editorOpen = true;
  editorDialog = new TaskEditorDialog(this);
  editorDialog->setTask(task, creating);

  connect(editorDialog, &TaskEditorDialog::saveRequested, this,
          [this](const ScheduledTask &draft, bool creatingTask) { submitTask(draft, creatingTask); });

  connect(editorDialog, &QDialog::finished, this, [this](int) {
    editorOpen = false;
    if (editorDialog) {
      editorDialog->deleteLater();
      editorDialog = nullptr;
    }
  });

  editorDialog->show();
}

void MainWindow::submitTask(const ScheduledTask &taskInput, bool creating) {
  ScheduledTask task = taskInput;
  if (!task.nextExecution.has_value()) {
    task.nextExecution = QDateTime::currentDateTimeUtc();
  }

  opInProgress = true;
  opLabel = creating ? "Creating task" : "Updating task";
  updateStatusBar();

  if (editorDialog) {
    editorDialog->setSaving(true);
  }

  if (creating) {
    api->createTask(task);
  } else {
    api->updateTask(task);
  }
}

void MainWindow::setStatus(const QString &text) {
  statusText = text;
  errorText.clear();
  updateStatusBar();
  statusClearTimer->start();
}

void MainWindow::setError(const QString &text) {
  errorText = text;
  statusText.clear();
  updateStatusBar();
}

void MainWindow::updateStatusBar() {
  QString line = "Ready.";
  QString color = "#7f8c8d";

  if (!errorText.isEmpty()) {
    line = QString("Error: %1").arg(errorText);
    color = "#c0392b";
  } else if (opInProgress || loading) {
    const QString spinner = spinnerFrames[spinnerIndex % spinnerFrames.size()];
    const QString label = opLabel.isEmpty() ? "Working" : opLabel;
    line = QString("%1 %2...").arg(spinner).arg(label);
    color = "#f39c12";
  } else if (!statusText.isEmpty()) {
    line = statusText;
    color = "#27ae60";
  }

  statusLabel->setText(line);
  statusLabel->setStyleSheet(QString("color: %1;").arg(color));
}
