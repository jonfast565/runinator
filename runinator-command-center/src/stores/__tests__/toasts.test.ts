import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { appService } from "../../core/services";
import { useAppStore } from "../app";

describe("feedback toasts", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.stubGlobal("window", {
      clearTimeout: () => undefined,
      setTimeout: () => 0,
    });
    appService.resetForTests();
  });

  it("setStatus pushes a success toast and keeps statusText intact", () => {
    const app = useAppStore();

    app.setStatus("Task saved");

    expect(app.statusText).toBe("Task saved");
    expect(app.toasts).toHaveLength(1);
    expect(app.toasts[0]).toMatchObject({ kind: "success", text: "Task saved" });
  });

  it("setError pushes an error toast and preserves errorText as the success signal", () => {
    const app = useAppStore();

    app.setError("Boom");

    expect(app.errorText).toBe("Boom");
    expect(app.toasts.at(-1)).toMatchObject({ kind: "error", text: "Boom" });
  });

  it("loading toasts render as a neutral kind, never success", () => {
    const app = useAppStore();

    const id = app.pushToast("loading", "Working...");

    expect(app.toasts[0]).toMatchObject({ id, kind: "loading", text: "Working..." });
    expect(app.toasts.some((toast) => toast.kind === "success")).toBe(false);
  });

  it("caps the stack at four toasts, dropping the oldest", () => {
    const app = useAppStore();

    for (let i = 0; i < 6; i += 1) {
      app.pushToast("info", `msg ${String(i)}`);
    }

    expect(app.toasts).toHaveLength(4);
    expect(app.toasts[0].text).toBe("msg 2");
    expect(app.toasts.at(-1)?.text).toBe("msg 5");
  });

  it("dismissToast removes a specific toast by id", () => {
    const app = useAppStore();

    const first = app.pushToast("info", "one");
    app.pushToast("info", "two");
    app.dismissToast(first);

    expect(app.toasts).toHaveLength(1);
    expect(app.toasts[0].text).toBe("two");
  });
});
