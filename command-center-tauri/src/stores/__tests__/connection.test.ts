import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useAppStore } from "../app";
import { useProvidersStore } from "../providers";
import { useResourcesStore } from "../resources";
import { useSecretsStore } from "../secrets";
import { useWorkflowsStore } from "../workflows";

describe("service connection state", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.stubGlobal("window", {
      clearTimeout: () => undefined,
      setTimeout: () => 0
    });
  });

  it("clears service reachability when discovery reports null", () => {
    const app = useAppStore();

    app.initialLoading = false;
    app.setServiceUrl("http://127.0.0.1:3000");
    expect(app.serviceBlocked).toBe(false);

    app.setServiceUrl(null);

    expect(app.serviceUrl).toBeNull();
    expect(app.backendReachable).toBe(false);
    expect(app.serviceConnected).toBe(false);
    expect(app.serviceBlocked).toBe(true);
    expect(app.loadingMessage).toBe("Waiting for Runinator service...");
  });

  it("clears stale backend-backed store state on disconnect", () => {
    const providers = useProvidersStore();
    const resources = useResourcesStore();
    const secrets = useSecretsStore();
    const workflows = useWorkflowsStore();

    providers.providers = [{ name: "console", actions: [], metadata: { credential_scopes: [], contract: null } }];
    resources.resourceRecords = [{ id: 1, provider: "jira" }];
    resources.selectedResourceRecord = resources.resourceRecords[0];
    secrets.secrets = [{ scope: "github", name: "default" }];
    secrets.selectSecret(secrets.secrets[0]);
    workflows.workflows = [{ ...workflows.workflowDraft, id: 7, name: "Stale Workflow" }];
    workflows.workflowRuns = [{ id: 9, status: "running", created_at: "", started_at: null, finished_at: null }];
    workflows.selectedWorkflowRunId = 9;

    providers.clearProviders();
    resources.clearResources();
    secrets.clearSecrets();
    workflows.clearServiceState();

    expect(providers.providers).toEqual([]);
    expect(resources.resourceRecords).toEqual([]);
    expect(resources.selectedResourceRecord).toBeNull();
    expect(secrets.secrets).toEqual([]);
    expect(secrets.selectedSecretKey).toBe("");
    expect(workflows.workflows).toEqual([]);
    expect(workflows.workflowRuns).toEqual([]);
    expect(workflows.selectedWorkflowRunId).toBe(0);
  });
});
