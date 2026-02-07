#include "main_window.h"

#include "ui_main_window.h"

#include <QColor>
#include <QHeaderView>
#include <QItemSelectionModel>
#include <QKeySequence>
#include <QShortcut>
#include <QSizePolicy>

MainWindow::MainWindow(QWidget *parent) : QMainWindow(parent), ui(new Ui::MainWindow) {
  ui->setupUi(this);
  setWindowTitle("Command Center");

  api = new ApiClient(this);
  discovery = new GossipDiscovery(this);

  setupUiBindings();
  setupShortcuts();

  connect(api, &ApiClient::tasksLoaded, this, &MainWindow::onTasksLoaded);
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

  connect(ui->tableView, &QTableView::doubleClicked, this, [this]() { editSelectedTask(); });

  ui->actionRefresh->setShortcut(QKeySequence(Qt::CTRL | Qt::Key_R));
  ui->actionAdd->setShortcut(QKeySequence(Qt::CTRL | Qt::Key_N));
  ui->actionEdit->setShortcut(QKeySequence(Qt::Key_E));
  ui->actionQuit->setShortcut(QKeySequence(Qt::Key_Q));
  ui->actionQuit->setShortcuts({QKeySequence(Qt::Key_Q), QKeySequence(Qt::Key_Escape)});

  connect(ui->actionRefresh, &QAction::triggered, this, &MainWindow::refreshTasks);
  connect(ui->actionRunNow, &QAction::triggered, this, &MainWindow::requestRunSelected);
  connect(ui->actionEdit, &QAction::triggered, this, &MainWindow::editSelectedTask);
  connect(ui->actionAdd, &QAction::triggered, this, &MainWindow::addNewTask);
  connect(ui->actionQuit, &QAction::triggered, this, &QWidget::close);

  statusLabel = new QLabel("Ready.", this);
  statusLabel->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
  serviceLabel = new QLabel("No service discovered", this);

  statusBar()->addWidget(statusLabel, 1);
  statusBar()->addPermanentWidget(serviceLabel);

  connect(ui->tableView->selectionModel(), &QItemSelectionModel::selectionChanged, this,
          &MainWindow::updateRunNowState);

  updateRunNowState();
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

void MainWindow::onTasksLoaded(const QVector<ScheduledTask> &loaded) {
  loading = false;
  opInProgress = false;
  opLabel.clear();

  tasks = loaded;
  updateTable();
  setStatus("Refreshed.");
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

int MainWindow::selectedRow() const {
  const QModelIndexList selection = ui->tableView->selectionModel()->selectedRows();
  if (selection.isEmpty()) {
    return -1;
  }
  return selection.first().row();
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

void MainWindow::addNewTask() {
  ScheduledTask task;
  task.enabled = true;
  task.timeout = 0;
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
