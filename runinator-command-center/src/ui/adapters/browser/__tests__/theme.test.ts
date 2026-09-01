import { afterEach, describe, expect, it, vi } from "vitest";
import { applyTheme } from "../theme";

interface MockMediaQueryList {
  matches: boolean;
  addEventListener: ReturnType<typeof vi.fn>;
  removeEventListener: ReturnType<typeof vi.fn>;
  addListener: ReturnType<typeof vi.fn>;
  removeListener: ReturnType<typeof vi.fn>;
}

function installBrowser(matches: boolean, modern = true) {
  const media: MockMediaQueryList = {
    matches,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
  };
  const setAttribute = vi.fn();

  if (!modern) {
    Object.assign(media, { addEventListener: undefined, removeEventListener: undefined });
  }

  vi.stubGlobal("window", { matchMedia: vi.fn(() => media) });
  vi.stubGlobal("document", { documentElement: { setAttribute } });

  return { media, setAttribute };
}

afterEach(() => {
  applyTheme("light");
  vi.unstubAllGlobals();
});

describe("applyTheme", () => {
  it("resolves system mode from the browser color-scheme preference", () => {
    const { media, setAttribute } = installBrowser(true);

    applyTheme("system");

    expect(setAttribute).toHaveBeenCalledWith("data-theme", "dark");
    expect(media.addEventListener).toHaveBeenCalledWith("change", expect.any(Function));

    media.matches = false;
    const onChange = media.addEventListener.mock.calls[0]?.[1] as () => void;
    onChange();
    expect(setAttribute).toHaveBeenLastCalledWith("data-theme", "light");
  });

  it("supports browsers with the legacy MediaQueryList listener API", () => {
    const { media, setAttribute } = installBrowser(true, false);

    applyTheme("system");

    expect(setAttribute).toHaveBeenCalledWith("data-theme", "dark");
    expect(media.addListener).toHaveBeenCalledWith(expect.any(Function));
  });

  it("stops following the system when an explicit theme is selected", () => {
    const { media, setAttribute } = installBrowser(true);

    applyTheme("system");
    applyTheme("light");

    expect(media.removeEventListener).toHaveBeenCalledWith("change", expect.any(Function));
    expect(setAttribute).toHaveBeenLastCalledWith("data-theme", "light");
  });
});
