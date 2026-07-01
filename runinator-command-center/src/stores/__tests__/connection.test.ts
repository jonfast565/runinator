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
    app.setReplicaState(
      [
        {
          replica_id: "00000000-0000-0000-0000-000000000001",
          replica_type: "worker",
          instance_id: "worker-1",
          runtime_id: "runtime",
          status: "live",
          attributes: {},
          first_seen_at: "",
          last_heartbeat_at: "",
          last_seen_at: ""
        }
      ],
      { workers: 1, wakers: 0, webservices: 0 }
    );
    expect(app.serviceBlocked).toBe(false);

    app.setServiceUrl(null);

    expect(app.serviceUrl).toBeNull();
    expect(app.backendReachable).toBe(false);
    expect(app.serviceConnected).toBe(false);
    expect(app.serviceBlocked).toBe(true);
    expect(app.loadingMessage).toBe("Waiting for Runinator service...");
    expect(app.replicas).toEqual([]);
    expect(app.replicaCounts).toEqual({ workers: 0, wakers: 0, webservices: 0 });
  });

  it("counts only live worker replicas when backend counts are absent", () => {
    const app = useAppStore();

    app.setReplicaState([
      {
        replica_id: "00000000-0000-0000-0000-000000000001",
        replica_type: "worker",
        instance_id: "worker-live-a",
        runtime_id: "runtime-a",
        status: "live",
        attributes: {},
        first_seen_at: "",
        last_heartbeat_at: "",
        last_seen_at: ""
      },
      {
        replica_id: "00000000-0000-0000-0000-000000000002",
        replica_type: "worker",
        instance_id: "worker-stale",
        runtime_id: "runtime-b",
        status: "stale",
        attributes: {},
        first_seen_at: "",
        last_heartbeat_at: "",
        last_seen_at: ""
      },
      {
        replica_id: "00000000-0000-0000-0000-000000000003",
        replica_type: "worker",
        instance_id: "worker-live-b",
        runtime_id: "runtime-c",
        status: "live",
        attributes: {},
        first_seen_at: "",
        last_heartbeat_at: "",
        last_seen_at: ""
      },
      {
        replica_id: "00000000-0000-0000-0000-000000000004",
        replica_type: "webservice",
        instance_id: "ws-live",
        runtime_id: "runtime-d",
        status: "live",
        attributes: {},
        first_seen_at: "",
        last_heartbeat_at: "",
        last_seen_at: ""
      }
    ]);

    expect(app.replicaCounts).toEqual({ workers: 2, wakers: 0, webservices: 1 });
    expect(app.liveReplicaCount).toBe(3);
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
    workflows.workflows = [{ ...workflows.workflowDraft, id: "00000000-0000-0000-0000-000000000007", name: "Stale Workflow" }];
    workflows.workflowRuns = [{ id: "00000000-0000-0000-0000-000000000009", status: "running", created_at: "", started_at: null, finished_at: null }];
    workflows.selectedWorkflowRunId = "00000000-0000-0000-0000-000000000009";

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
    expect(workflows.selectedWorkflowRunId).toBeNull();
  });
});
