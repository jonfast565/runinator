import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useKeyboardShortcuts } from "../useKeyboardShortcuts";
import { useWorkflowsStore } from "../../stores/workflows";

describe("useKeyboardShortcuts", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("does not intercept keys typed in CodeMirror-like editors", () => {
    const workflows = useWorkflowsStore();
    const runSelectedWorkflow = vi.spyOn(workflows, "runSelectedWorkflow");
    const { handleKeydown } = useKeyboardShortcuts();
    const preventDefault = vi.fn();
    const target = {
      tagName: "DIV",
      isContentEditable: false,
      closest: vi.fn((selectors: string) => (selectors.includes(".cm-editor") ? {} : null))
    } as unknown as EventTarget;

    handleKeydown({ key: "e", target, preventDefault } as unknown as KeyboardEvent);
    handleKeydown({ key: "Enter", target, preventDefault } as unknown as KeyboardEvent);

    expect(preventDefault).not.toHaveBeenCalled();
    expect(runSelectedWorkflow).not.toHaveBeenCalled();
  });

  it("still blocks the e shortcut outside editable surfaces", () => {
    const { handleKeydown } = useKeyboardShortcuts();
    const preventDefault = vi.fn();
    const target = {
      tagName: "DIV",
      isContentEditable: false,
      closest: vi.fn(() => null)
    } as unknown as EventTarget;

    handleKeydown({ key: "e", target, preventDefault } as unknown as KeyboardEvent);

    expect(preventDefault).toHaveBeenCalledOnce();
  });
});
