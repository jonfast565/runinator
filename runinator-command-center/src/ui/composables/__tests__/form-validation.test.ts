import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  applyDefaultConstraints,
  validateFormControl,
  validateFormControls,
  type FormControl,
} from "../form-validation";

class FakeControl {
  dataset: DOMStringMap = {};
  disabled = false;
  maxLength = -1;
  required = false;
  value = "";
  willValidate = true;
  customValidity = "";
  nativeValid = true;

  get validity(): ValidityState {
    return { valid: this.nativeValid && !this.customValidity } as ValidityState;
  }

  closest(): Element | null {
    return null;
  }

  setCustomValidity(message: string): void {
    this.customValidity = message;
  }
}

class FakeInput extends FakeControl {
  step = "";
  type = "text";
}

class FakeSelect extends FakeControl {}

class FakeTextarea extends FakeControl {}

function asControl(control: FakeControl): FormControl {
  return control as unknown as FormControl;
}

function rootWith(...controls: FakeControl[]): ParentNode {
  return {
    querySelectorAll: () => controls,
  } as unknown as ParentNode;
}

describe("form validation", () => {
  beforeEach(() => {
    vi.stubGlobal("HTMLInputElement", FakeInput);
    vi.stubGlobal("HTMLSelectElement", FakeSelect);
    vi.stubGlobal("HTMLTextAreaElement", FakeTextarea);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("adds bounded defaults to every free-text control", () => {
    const text = new FakeInput();
    const password = new FakeInput();
    password.type = "password";
    const url = new FakeInput();
    url.type = "url";
    const textarea = new FakeTextarea();

    applyDefaultConstraints(rootWith(text, password, url, textarea));

    expect(text.maxLength).toBe(256);
    expect(password.maxLength).toBe(16 * 1024);
    expect(url.maxLength).toBe(2 * 1024);
    expect(textarea.maxLength).toBe(16 * 1024);
  });

  it("rejects whitespace-only required values and oversized programmatic values", () => {
    const required = new FakeInput();
    required.required = true;
    required.value = "   ";
    expect(validateFormControl(asControl(required))).toBe(false);
    expect(required.customValidity).toContain("not only whitespace");

    const oversized = new FakeInput();
    oversized.maxLength = 3;
    oversized.value = "four";
    expect(validateFormControl(asControl(oversized))).toBe(false);
    expect(oversized.customValidity).toBe("Use at most 3 characters.");
  });

  it("validates semantic JSON, identifier, UUID, and HTTP URL fields", () => {
    const json = new FakeTextarea();
    json.dataset.validation = "json";
    json.value = "{";
    expect(validateFormControl(asControl(json))).toBe(false);

    const identifier = new FakeInput();
    identifier.dataset.validation = "identifier";
    identifier.value = "has spaces";
    expect(validateFormControl(asControl(identifier))).toBe(false);

    const uuid = new FakeInput();
    uuid.dataset.validation = "uuid";
    uuid.value = "not-a-uuid";
    expect(validateFormControl(asControl(uuid))).toBe(false);

    const url = new FakeInput();
    url.type = "url";
    url.value = "file:///tmp/config";
    expect(validateFormControl(asControl(url))).toBe(false);
    expect(url.customValidity).toContain("http:// or https://");
  });

  it("checks every untouched control at submission time and returns the first invalid field", () => {
    const first = new FakeInput();
    first.required = true;
    first.value = "  ";
    const second = new FakeTextarea();
    second.dataset.validation = "json";
    second.value = "[";

    expect(validateFormControls(rootWith(first, second))).toBe(asControl(first));
    expect(first.customValidity).not.toBe("");
    expect(second.customValidity).not.toBe("");
  });
});
