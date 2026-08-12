import { describe, expect, it } from "vitest";
import { buildInfo, buildTooltip, versionLabel, type BuildInfo } from "../build-info";

const stamp: BuildInfo = {
  version: "1.2.3",
  buildId: "a1b2c3d",
  builtAt: "2026-08-12T09:30:00.000Z",
};

describe("build info", () => {
  it("injects a version at build time", () => {
    expect(buildInfo.version).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("labels version and build id together", () => {
    expect(versionLabel(stamp)).toBe("v1.2.3 · a1b2c3d");
  });

  it("drops the build id when the build was not stamped", () => {
    expect(versionLabel({ ...stamp, buildId: "" })).toBe("v1.2.3");
  });

  it("spells the stamp out in the tooltip", () => {
    const lines = buildTooltip(stamp).split("\n");
    expect(lines[0]).toBe("Command Center v1.2.3");
    expect(lines[1]).toBe("Build a1b2c3d");
    expect(lines[2]).toMatch(/^Built /);
  });

  it("omits an unparseable build time rather than printing Invalid Date", () => {
    expect(buildTooltip({ ...stamp, builtAt: "not-a-date" })).toBe(
      "Command Center v1.2.3\nBuild a1b2c3d",
    );
  });
});
