import { useAppStore } from "../stores/app";
import { useResourcesStore } from "../stores/resources";
import { useSecretsStore } from "../stores/secrets";
import { useTasksStore } from "../stores/tasks";
import { useWorkflowsStore } from "../stores/workflows";

export function useKeyboardShortcuts() {
  const app = useAppStore();
  const tasks = useTasksStore();
  const workflows = useWorkflowsStore();
  const resources = useResourcesStore();
  const secrets = useSecretsStore();

  function handleKeydown(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    const editing = ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
    if (editing) return;
    if (event.key === "/") {
      event.preventDefault();
      document.getElementById("global-search")?.focus();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      moveSelection(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveSelection(-1);
    } else if (event.key === "r" || (event.ctrlKey && event.key.toLowerCase() === "r")) {
      event.preventDefault();
      refreshActive();
    } else if (event.key === "Enter") {
      event.preventDefault();
      tasks.runSelectedTask();
    } else if (event.ctrlKey && event.key.toLowerCase() === "n") {
      event.preventDefault();
      tasks.openNewTask();
    } else if (event.key.toLowerCase() === "e") {
      event.preventDefault();
      tasks.openSelectedTask();
    }
  }

  async function refreshActive() {
    if (app.activeTab === "Tasks") await tasks.refreshTasks();
    else if (app.activeTab === "Runs") await tasks.refreshRunsForSelectedTask();
    else if (app.activeTab === "Workflows") await workflows.refreshWorkflows();
    else if (app.activeTab === "Resources") await resources.refreshResources();
    else await secrets.refreshSecrets();
  }

  function moveSelection(delta: number) {
    if (app.activeTab === "Tasks") {
      tasks.moveTaskSelection(delta);
    } else if (app.activeTab === "Runs") {
      tasks.moveRunSelection(delta);
    } else if (app.activeTab === "Workflows") {
      workflows.moveWorkflowSelection(delta);
    } else if (app.activeTab === "Resources") {
      resources.moveResourceSelection(delta);
    } else {
      secrets.moveSecretSelection(delta);
    }
  }

  return { handleKeydown, refreshActive };
}
