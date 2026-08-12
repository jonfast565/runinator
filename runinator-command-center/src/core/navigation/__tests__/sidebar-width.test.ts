import { describe, expect, it } from "vitest";
import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  clampSidebarWidth,
  parseSidebarWidth,
} from "../sidebar-width";

describe("sidebar width", () => {
  it("keeps a width inside the rail's bounds", () => {
    expect(clampSidebarWidth(260, 1600)).toBe(260);
    expect(clampSidebarWidth(40, 1600)).toBe(SIDEBAR_MIN_WIDTH);
    expect(clampSidebarWidth(9000, 1600)).toBe(SIDEBAR_MAX_WIDTH);
  });

  it("caps the rail at 40% of a narrow viewport", () => {
    expect(clampSidebarWidth(400, 900)).toBe(360);
  });

  it("never clamps below the minimum, even on a viewport too small for it", () => {
    expect(clampSidebarWidth(300, 200)).toBe(SIDEBAR_MIN_WIDTH);
  });

  it("falls back to the default for missing or junk storage", () => {
    expect(parseSidebarWidth(null)).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(parseSidebarWidth("")).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(parseSidebarWidth("wide")).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(parseSidebarWidth("-40")).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it("clamps a stored width against the current viewport", () => {
    expect(parseSidebarWidth("300", 1600)).toBe(300);
    expect(parseSidebarWidth("300", 600)).toBe(240);
    expect(parseSidebarWidth("300", 300)).toBe(SIDEBAR_MIN_WIDTH);
  });
});
