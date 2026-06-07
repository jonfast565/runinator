import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { effectScope, nextTick } from "vue";
import { useWorkflowRunStream } from "../useWorkflowRunStream";
import { useAppStore } from "../../stores/app";
import { useWorkflowsStore } from "../../stores/workflows";
import type { WorkflowRunDetail } from "../../types/models";

const RUN_ID = "00000000-0000-0000-0000-000000000042";
const WORKFLOW_ID = "00000000-0000-0000-0000-000000000007";

class MockWebSocket {
  static sockets: MockWebSocket[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  close = vi.fn(() => {
    this.onclose?.();
  });

  constructor(public readonly url: string) {
    MockWebSocket.sockets.push(this);
  }
}

describe("useWorkflowRunStream", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    setActivePinia(createPinia());
    MockWebSocket.sockets = [];
    vi.stubGlobal("WebSocket", MockWebSocket);
    vi.stubGlobal("window", {
      clearTimeout,
      setTimeout
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("does not reconnect after a terminal workflow run detail", async () => {
    const app = useAppStore();
    const workflows = useWorkflowsStore();
    app.setServiceUrl("http://127.0.0.1:8080/");
    workflows.openRunInTab(RUN_ID);
    workflows.activateRunTab(RUN_ID);

    const scope = effectScope();
    scope.run(() => useWorkflowRunStream());
    await nextTick();

    const socket = MockWebSocket.sockets[0];
    expect(socket.url).toBe(`ws://127.0.0.1:8080/ws/workflow-runs/${RUN_ID}`);

    socket.onmessage?.({ data: JSON.stringify(workflowDetail(RUN_ID, "succeeded")) });
    socket.onclose?.();
    vi.advanceTimersByTime(3000);

    expect(workflows.workflowRunDetail?.run.status).toBe("succeeded");
    expect(MockWebSocket.sockets).toHaveLength(1);
    scope.stop();
  });
});

function workflowDetail(id: string, status: string): WorkflowRunDetail {
  return {
    run: {
      id,
      workflow_id: WORKFLOW_ID,
      status,
      parameters: {},
      state: {},
      active_node_id: null,
      created_at: "2026-01-01T00:00:00Z",
      started_at: null,
      finished_at: status === "succeeded" ? "2026-01-01T00:01:00Z" : null,
      message: null
    },
    nodes: []
  };
}
