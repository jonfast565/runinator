import { beforeEach, describe, expect, it, vi } from "vitest";
import { createConsoleService } from "../console";
import type { ConsoleCell, ConsoleSessionDetail } from "../../domain/models";
import type { AppService } from "../app";

vi.mock("../../api/commandCenterApi", () => ({
  fetchConsoleSessions: vi.fn(),
  fetchConsoleSession: vi.fn(),
  createConsoleSession: vi.fn(),
  renameConsoleSession: vi.fn(),
  deleteConsoleSession: vi.fn(),
  createConsoleCell: vi.fn(),
  fetchConsoleCell: vi.fn(),
  updateConsoleCell: vi.fn(),
  deleteConsoleCell: vi.fn(),
  runConsoleCell: vi.fn(),
}));

import {
  fetchConsoleCell,
  fetchConsoleSession,
  fetchConsoleSessions,
  runConsoleCell,
} from "../../api/commandCenterApi";

function cell(overrides: Partial<ConsoleCell> = {}): ConsoleCell {
  return {
    id: "cell-1",
    session_id: "session-1",
    position: 0,
    label: null,
    source: "1 + 2",
    kind: null,
    status: "idle",
    result: null,
    error: null,
    workflow_run_id: null,
    created_at: "2026-08-16T00:00:00Z",
    updated_at: "2026-08-16T00:00:00Z",
    ...overrides,
  };
}

function session(cells: ConsoleCell[] = []): ConsoleSessionDetail {
  return {
    id: "session-1",
    org_id: null,
    name: "scratch",
    created_by: null,
    created_at: "2026-08-16T00:00:00Z",
    updated_at: "2026-08-16T00:00:00Z",
    cells,
    bindings: [],
  };
}

// a minimal app stub: runOperation passes the call through so rejections reach the service.
const messages: string[] = [];
const app = {
  runOperation: <T>(_label: string, run: () => Promise<T>) => run(),
  setError: (message: string) => {
    messages.push(message);
  },
  setStatus: (message: string) => {
    messages.push(message);
  },
} as unknown as AppService;

describe("console cell naming", () => {
  it("falls back to a positional name for a blank label", async () => {
    const { cellBindingName, cellReference } = await import("../../domain/models");

    expect(cellBindingName(cell({ label: "total" }))).toBe("total");
    expect(cellBindingName(cell({ label: null }))).toBe("cell_0");
    // a whitespace-only label is not a label. the backend filters it the same way, and a mismatch
    // This would make the UI show a binding name that resolves to nothing.
    expect(cellBindingName(cell({ label: "   " }))).toBe("cell_0");
    expect(cellReference(cell({ label: "   ", position: 2 }))).toBe("params.cell_2");
  });
});

describe("console service", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(fetchConsoleSessions).mockResolvedValue([session()]);
    vi.mocked(fetchConsoleSession).mockResolvedValue(session());
  });

  it("does not poll a cell that already settled", async () => {
    // the console's whole point is that a pure cell answers in one request. polling it anyway would
    // add a second of latency to `1 + 2` and a request per cell run for no reason.
    const service = createConsoleService(app);
    await service.openSession("session-1");
    vi.mocked(runConsoleCell).mockResolvedValue(
      cell({ status: "succeeded", kind: "expression", result: 3 }),
    );

    await service.runCell("cell-1");

    expect(vi.mocked(fetchConsoleCell)).not.toHaveBeenCalled();
    expect(service.isPending("cell-1")).toBe(false);
  });

  it("marks an effectful cell pending so the view can show it as busy", async () => {
    const service = createConsoleService(app);
    await service.openSession("session-1");
    vi.mocked(runConsoleCell).mockResolvedValue(
      cell({ status: "running", kind: "workflow", workflow_run_id: "run-1" }),
    );
    // never settles within the test; the point is only that the follow starts.
    vi.mocked(fetchConsoleCell).mockResolvedValue(
      cell({ status: "running", kind: "workflow", workflow_run_id: "run-1" }),
    );

    const result = await service.runCell("cell-1");

    expect(result.status).toBe("running");
    expect(service.isPending("cell-1")).toBe(true);
  });

  it("resumes following a cell that was still running when the session was reopened", async () => {
    // a reload must not strand a cell showing `running` with nothing watching it.
    vi.mocked(fetchConsoleSession).mockResolvedValue(
      session([cell({ status: "running", workflow_run_id: "run-1" })]),
    );
    vi.mocked(fetchConsoleCell).mockResolvedValue(
      cell({ status: "running", workflow_run_id: "run-1" }),
    );

    const service = createConsoleService(app);
    await service.openSession("session-1");

    expect(service.isPending("cell-1")).toBe(true);
  });

  it("keeps the run's cell in state when it settles", async () => {
    const service = createConsoleService(app);
    await service.openSession("session-1");
    vi.mocked(fetchConsoleSession).mockResolvedValue(session([cell()]));
    await service.refreshActiveSession();

    const settled = cell({ status: "succeeded", kind: "expression", result: 3 });
    vi.mocked(runConsoleCell).mockResolvedValue(settled);
    // a pure run ends with a session refresh to pick up the new binding, and the backend has
    // already written the cell by then — so the refreshed session carries the settled cell too.
    vi.mocked(fetchConsoleSession).mockResolvedValue(session([settled]));
    await service.runCell("cell-1");

    const stored = service.getState().activeSession?.cells?.at(0);
    expect(stored?.status).toBe("succeeded");
    expect(stored?.result).toBe(3);
  });
});
