import { endpointForTab, isResourceTab, useAppStore } from "../../ui/adapters/pinia/app";
import { useResourcesStore } from "../../ui/adapters/pinia/resources";
import { useSecretsStore } from "../../ui/adapters/pinia/secrets";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";

export function useKeyboardShortcuts() {
  const app = useAppStore();
  const workflows = useWorkflowsStore();
  const resources = useResourcesStore();
  const secrets = useSecretsStore();

  function handleKeydown(event: KeyboardEvent) {
    const editing = isEditableTarget(event.target);

    // debug shortcuts intentionally run even while editing as long as the editor
    // doesn't swallow them — CodeMirror handles its own focus.
    if (event.key === "F5") {
      event.preventDefault();

      if (event.shiftKey) {
        void workflows.cancelSelectedWorkflowRun();
      } else {
        void workflows.continueSelectedWorkflowRun();
      }

      return;
    }

    if (event.key === "F10") {
      event.preventDefault();

      if (event.ctrlKey) {
        const nodeId = workflows.selectedWorkflowRunNodeId;

        if (nodeId) {
          void workflows.runToCursor(nodeId);
        }
      } else {
        void workflows.stepSelectedWorkflowRun();
      }

      return;
    }

    if (event.key === "F9") {
      event.preventDefault();
      const nodeId = workflows.selectedWorkflowRunNodeId;

      if (nodeId) {
        void workflows.toggleBreakpoint(nodeId);
      }

      return;
    }

    if (editing) {
      return;
    }

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
      void refreshActive();
    } else if (event.key === "Enter") {
      event.preventDefault();

      if (app.activeTab === "Workflows") {
        void workflows.runSelectedWorkflow();
      }
    } else if (event.ctrlKey && event.key.toLowerCase() === "n") {
      event.preventDefault();
    } else if (event.key.toLowerCase() === "e") {
      event.preventDefault();
    }
  }

  async function refreshActive() {
    if (app.activeTab === "Runs") {
      await workflows.fetchRecentWorkflowRuns();
    } else if (app.activeTab === "Workflows") {
      await workflows.refreshWorkflows();
    } else if (app.activeTab === "Secrets") {
      await secrets.refreshSecrets();
    } else if (isResourceTab(app.activeTab)) {
      const endpoint = endpointForTab(app.activeTab);

      if (endpoint) {
        await resources.refreshResourcesFor(endpoint);
      }
    }
  }

  function moveSelection(delta: number) {
    if (app.activeTab === "Workflows") {
      workflows.moveWorkflowSelection(delta);
    } else if (isResourceTab(app.activeTab)) {
      resources.moveResourceSelection(delta);
    } else if (app.activeTab === "Secrets") {
      secrets.moveSecretSelection(delta);
    }
  }

  return { handleKeydown, refreshActive };
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || typeof target !== "object") {
    return false;
  }

  const element = target as HTMLElement & {
    isContentEditable?: boolean;
    closest?: (selectors: string) => Element | null;
    tagName?: string;
  };

  if (typeof element.tagName !== "string") {
    return false;
  }

  if (element.isContentEditable) {
    return true;
  }

  if (element.closest(".cm-editor, [contenteditable='true']")) {
    return true;
  }

  return ["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName);
}
