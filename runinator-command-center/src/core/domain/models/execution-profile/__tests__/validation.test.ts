import { describe, expect, it } from "vitest";
import type { ExecutionProfileInput } from "..";
import { validateExecutionProfile } from "../validation";

function validProfile(): ExecutionProfileInput {
  return {
    name: "github-default",
    description: "GitHub CLI session",
    credential_scopes: ["github", "copilot"],
    collection: {
      version: 1,
      probe: { argv: ["gh", "auth", "status"] },
      refresh: { argv: ["gh", "auth", "login"], interactive: true },
      sources: [{ type: "directory", path: "~/.config/gh", glob: "*", target: ".config/gh" }],
    },
    exposure: {
      version: 1,
      home_overlay: true,
      environment: { GH_CONFIG_DIR: "${PROFILE_HOME}/.config/gh" },
    },
    enabled: true,
  };
}

describe("execution profile validation", () => {
  it("accepts a complete provider-agnostic profile", () => {
    expect(validateExecutionProfile(validProfile())).toEqual({
      fields: {},
      valid: true,
      summary: "Profile configuration is valid.",
    });
  });

  it("reports source errors at the source field", () => {
    const profile = validProfile();
    profile.collection.sources = [
      { type: "file", path: "", target: "../credentials" },
      { type: "file", path: "~/.aws/config", target: "../credentials" },
    ];
    const result = validateExecutionProfile(profile);

    expect(result.fields["sources.0.path"]).toBeTruthy();
    expect(result.fields["sources.0.target"]).toBeTruthy();
    expect(result.fields["sources.1.target"]).toContain("unique");
  });

  it("rejects shell-like gaps and unsafe environment templates", () => {
    const profile = validProfile();
    profile.collection.probe = { argv: ["gh", ""] };
    profile.exposure.environment = {
      "BAD-NAME": "/tmp/token",
      CONFIG: "${HOME}/config",
    };
    const result = validateExecutionProfile(profile);

    expect(result.fields.probe).toBeTruthy();
    expect(result.fields["environment.BAD-NAME.name"]).toBeTruthy();
    expect(result.fields["environment.BAD-NAME.value"]).toBeTruthy();
    expect(result.fields["environment.CONFIG.value"]).toBeTruthy();
  });
});
