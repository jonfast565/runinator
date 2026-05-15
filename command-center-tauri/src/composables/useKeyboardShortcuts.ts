import { useAppStore } from "../stores/app";
import { useResourcesStore } from "../stores/resources";
import { useSecretsStore } from "../stores/secrets";
import { useWorkflowsStore } from "../stores/workflows";

export function useKeyboardShortcuts() {
  const app = useAppStore();
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
      if (app.activeTab === "Workflows") workflows.runSelectedWorkflow();
    } else if (event.ctrlKey && event.key.toLowerCase() === "n") {
      event.preventDefault();
    } else if (event.key.toLowerCase() === "e") {
      event.preventDefault();
    }
  }

  async function refreshActive() {
    if (app.activeTab === "Runs") await workflows.fetchRecentWorkflowRuns();
    else if (app.activeTab === "Workflows") await workflows.refreshWorkflows();
    else if (app.activeTab === "Resources") await resources.refreshResources();
    else await secrets.refreshSecrets();
  }

  function moveSelection(delta: number) {
    if (app.activeTab === "Runs") {
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
