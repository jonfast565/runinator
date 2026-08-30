import { afterEach, describe, expect, it, vi } from "vitest";
import { createUuid } from "../uuid";

describe("createUuid", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("uses randomUUID when the runtime provides it", () => {
    const expected = "123e4567-e89b-42d3-a456-426614174000";
    vi.stubGlobal("crypto", { randomUUID: vi.fn(() => expected) });

    expect(createUuid()).toBe(expected);
  });

  it("builds a v4 UUID when randomUUID is unavailable", () => {
    vi.stubGlobal("crypto", {
      getRandomValues: (bytes: Uint8Array) => bytes.fill(0),
    });

    expect(createUuid()).toBe("00000000-0000-4000-8000-000000000000");
  });

  it("remains available without Web Crypto", () => {
    vi.stubGlobal("crypto", undefined);
    vi.spyOn(Math, "random").mockReturnValue(0);

    expect(createUuid()).toBe("00000000-0000-4000-8000-000000000000");
  });
});
