import { onBeforeUnmount, watch } from "vue";
import { useAppStore } from "../../ui/adapters/pinia/app";
import { useAuthStore } from "../../ui/adapters/pinia/auth";
import { useWorkflowsStore } from "../../ui/adapters/pinia/workflows";
import type { WorkflowRunDetail } from "../../core/domain/models";
import { isTerminalWorkflowRunStatus } from "../../core/utils/status";
import { buildWebSocketProtocols, buildWebSocketUrl } from "../../core/utils/websocket";
import { ReconnectBackoff } from "../../core/realtime/reconnect-backoff";

interface RunStreamHandle {
  socket: WebSocket | null;
  reconnectTimer: ReturnType<typeof setTimeout> | null;
  connectionId: number;
  terminal: boolean;
  disposed: boolean;
  backoff: ReconnectBackoff;
}

export function useWorkflowRunStream() {
  const workflows = useWorkflowsStore();
  const app = useAppStore();
  const auth = useAuthStore();
  const sockets = new Map<string, RunStreamHandle>();

  function disposeHandle(runId: string) {
    const handle = sockets.get(runId);

    if (!handle) {
      return;
    }

    handle.disposed = true;
    handle.connectionId += 1;

    if (handle.reconnectTimer) {
      clearTimeout(handle.reconnectTimer);
      handle.reconnectTimer = null;
    }

    if (handle.socket?.readyState === WebSocket.OPEN) {
      handle.socket.close();
    }

    handle.socket = null;
    sockets.delete(runId);
  }

  function ensureHandle(runId: string): RunStreamHandle {
    const existing = sockets.get(runId);

    if (existing) {
      return existing;
    }

    const handle: RunStreamHandle = {
      socket: null,
      reconnectTimer: null,
      connectionId: 0,
      terminal: false,
      disposed: false,
      backoff: new ReconnectBackoff(),
    };
    sockets.set(runId, handle);
    return handle;
  }

  function connect(runId: string) {
    if (!runId || !app.serviceUrl) {
      return;
    }

    const handle = ensureHandle(runId);

    if (handle.reconnectTimer) {
      clearTimeout(handle.reconnectTimer);
      handle.reconnectTimer = null;
    }

    const myConnectionId = ++handle.connectionId;
    const socket = new WebSocket(
      buildWebSocketUrl(app.serviceUrl, `/ws/workflow-runs/${runId}`),
      buildWebSocketProtocols(),
    );
    handle.socket = socket;

    socket.onopen = () => {
      if (handle.disposed || handle.connectionId !== myConnectionId) {
        socket.close();
        return;
      }

      handle.backoff.reset();
      console.info("[command-center] workflow run stream connected", { runId });
    };

    socket.onmessage = ({ data }: MessageEvent<string>) => {
      if (handle.disposed || handle.connectionId !== myConnectionId) {
        return;
      }

      try {
        const detail = JSON.parse(data) as WorkflowRunDetail;
        workflows.setWorkflowRunDetail(detail);
        // The run websocket is intentionally a small status envelope. Schedule an authoritative
        // VM detail read after every push so a newly opened tab gets its journal/effect projection
        // even when the initial status message races the first HTTP read.

        if (detail.nodes.length === 0) {
          workflows.scheduleWorkflowRunDetailRefresh(runId);
        }

        if (isTerminalWorkflowRunStatus(detail.run.status)) {
          handle.terminal = true;
        }
      } catch (err) {
        console.info("[command-center] failed to parse workflow run stream message", {
          runId,
          data,
          err,
        });
      }
    };

    socket.onerror = (event) => {
      if (handle.disposed || handle.connectionId !== myConnectionId) {
        return;
      }

      console.info("[command-center] workflow run stream error", { runId, event });

      if (socket.readyState === WebSocket.OPEN) {
        socket.close();
      }
    };

    socket.onclose = () => {
      if (handle.disposed || handle.connectionId !== myConnectionId) {
        return;
      }

      handle.socket = null;

      if (handle.terminal) {
        return;
      }

      if (workflows.openRunIds.includes(runId) && app.serviceKnown) {
        handle.reconnectTimer = setTimeout(() => {
          connect(runId);
        }, handle.backoff.next());
      }
    };
  }

  function syncSockets(ids: string[]) {
    const set = new Set(ids);

    for (const id of [...sockets.keys()]) {
      if (!set.has(id)) {
        disposeHandle(id);
      }
    }

    for (const id of ids) {
      if (!id) {
        continue;
      }

      if (!sockets.has(id)) {
        connect(id);
      }
    }
  }

  watch(
    () => [...workflows.openRunIds],
    (ids) => {
      syncSockets(ids);
    },
    { immediate: true, deep: true },
  );

  watch(
    () => app.serviceUrl,
    () => {
      for (const id of [...sockets.keys()]) {
        disposeHandle(id);
      }

      syncSockets([...workflows.openRunIds]);
    },
  );

  watch(
    () => auth.accessTokenRevision,
    () => {
      for (const id of [...sockets.keys()]) {
        disposeHandle(id);
      }

      syncSockets([...workflows.openRunIds]);
    },
  );

  function disposeAll() {
    for (const id of [...sockets.keys()]) {
      disposeHandle(id);
    }
  }

  onBeforeUnmount(disposeAll);
}
