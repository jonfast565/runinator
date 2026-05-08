#include "task_editor_dialog.h"

#include "utils/task_validator.h"

#include <QDialogButtonBox>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QJsonObject>
#include <QPushButton>

#include "ui_task_editor_dialog.h"

TaskEditorDialog::TaskEditorDialog(QWidget *parent)
    : QDialog(parent), ui(new Ui::TaskEditorDialog) {
  ui->setupUi(this);
  ui->errorLabel->setStyleSheet("color: #c0392b;");

  connect(ui->buttonBox, &QDialogButtonBox::accepted, this, &TaskEditorDialog::handleSave);
  connect(ui->buttonBox, &QDialogButtonBox::rejected, this, &TaskEditorDialog::reject);

  auto *saveShortcut = new QShortcut(QKeySequence::Save, this);
  connect(saveShortcut, &QShortcut::activated, this, &TaskEditorDialog::handleSave);
}

TaskEditorDialog::~TaskEditorDialog() { delete ui; }

void TaskEditorDialog::setTask(const ScheduledTask &task, bool creatingTask) {
  creating = creatingTask;
  setWindowTitle(creating ? "New Task" : "Edit Task");

  taskId = task.id;
  nextExecution = task.nextExecution;
  immediate = task.immediate;
  blackoutStart = task.blackoutStart;
  blackoutEnd = task.blackoutEnd;

  ui->nameEdit->setText(task.name);
  ui->cronEdit->setText(task.cronSchedule);
  ui->actionNameEdit->setText(task.actionName);
  ui->actionFunctionEdit->setText(task.actionFunction);
  ui->actionConfigEdit->setPlainText(task.actionConfiguration);
  ui->timeoutSpin->setValue(static_cast<int>(task.timeout));
  ui->enabledCheck->setChecked(task.enabled);
  ui->mcpEnabledCheck->setChecked(task.mcpEnabled);
  ui->inputSchemaEdit->setPlainText(
      QJsonDocument(task.inputSchema).toJson(QJsonDocument::Indented));
  ui->defaultParamsEdit->setPlainText(
      QJsonDocument(task.defaultParameters).toJson(QJsonDocument::Indented));
  ui->metadataEdit->setPlainText(QJsonDocument(task.metadata).toJson(QJsonDocument::Indented));
  setError(QString());
}

void TaskEditorDialog::setSaving(bool saving) {
  ui->nameEdit->setEnabled(!saving);
  ui->cronEdit->setEnabled(!saving);
  ui->actionNameEdit->setEnabled(!saving);
  ui->actionFunctionEdit->setEnabled(!saving);
  ui->actionConfigEdit->setEnabled(!saving);
  ui->timeoutSpin->setEnabled(!saving);
  ui->enabledCheck->setEnabled(!saving);
  ui->mcpEnabledCheck->setEnabled(!saving);
  ui->inputSchemaEdit->setEnabled(!saving);
  ui->defaultParamsEdit->setEnabled(!saving);
  ui->metadataEdit->setEnabled(!saving);
  ui->buttonBox->button(QDialogButtonBox::Save)->setEnabled(!saving);
  ui->buttonBox->button(QDialogButtonBox::Cancel)->setEnabled(!saving);
}

void TaskEditorDialog::setError(const QString &message) { ui->errorLabel->setText(message); }

ScheduledTask TaskEditorDialog::collectTask() const {
  ScheduledTask task;
  task.id = taskId;
  task.name = ui->nameEdit->text();
  task.cronSchedule = ui->cronEdit->text();
  task.actionName = ui->actionNameEdit->text();
  task.actionFunction = ui->actionFunctionEdit->text();
  task.actionConfiguration = ui->actionConfigEdit->toPlainText();
  task.timeout = ui->timeoutSpin->value();
  task.nextExecution = nextExecution;
  task.enabled = ui->enabledCheck->isChecked();
  task.mcpEnabled = ui->mcpEnabledCheck->isChecked();
  task.immediate = immediate;
  task.blackoutStart = blackoutStart;
  task.blackoutEnd = blackoutEnd;

  auto parseObject = [](const QString &text, const QJsonObject &fallback) {
    QJsonParseError error;
    const QJsonDocument doc = QJsonDocument::fromJson(text.toUtf8(), &error);
    if (error.error == QJsonParseError::NoError && doc.isObject()) {
      return doc.object();
    }
    return fallback;
  };
  task.inputSchema = parseObject(ui->inputSchemaEdit->toPlainText(), task.inputSchema);
  task.defaultParameters = parseObject(ui->defaultParamsEdit->toPlainText(), task.defaultParameters);
  task.metadata = parseObject(ui->metadataEdit->toPlainText(), task.metadata);
  return task;
}

void TaskEditorDialog::handleSave() {
  ScheduledTask task = collectTask();
  const QString err = validateTask(task);
  if (!err.isEmpty()) {
    setError(err);
    return;
  }
  for (const auto *editor : {ui->inputSchemaEdit, ui->defaultParamsEdit, ui->metadataEdit}) {
    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(editor->toPlainText().toUtf8(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
      setError("Schema, parameters, and metadata must be JSON objects");
      return;
    }
  }
  setError(QString());
  emit saveRequested(task, creating);
}
