import { describe, expect, it } from "vitest";
import { artifactIdentityError, artifactIdentityErrors, artifactIdentityPath } from "../identity";

describe("artifact identity", () => {
  it("accepts canonical namespaced stable keys", () => {
    const identity = {
      name: "Release pipeline",
      namespace: "acme.delivery",
      key: "release_train",
    };

    expect(artifactIdentityError(identity)).toBe("");
    expect(artifactIdentityPath(identity)).toBe("acme.delivery.release_train");
  });

  it("requires every identity field", () => {
    expect(artifactIdentityError({ name: "", namespace: "acme", key: "release" })).toBe(
      "Name is required.",
    );
    expect(artifactIdentityError({ name: "Release", namespace: "", key: "release" })).toContain(
      "Namespace is required",
    );
    expect(artifactIdentityError({ name: "Release", namespace: "acme", key: "" })).toBe(
      "Stable key is required.",
    );
  });

  it("rejects non-REXRAP namespace segments and keys", () => {
    expect(
      artifactIdentityError({ name: "Release", namespace: "acme.delivery-jobs", key: "release" }),
    ).toContain("Each namespace segment");
    expect(
      artifactIdentityError({ name: "Release", namespace: "acme.delivery", key: "release-job" }),
    ).toContain("Stable key must start");
  });

  it("assigns an error to the field that needs correction", () => {
    expect(
      artifactIdentityErrors({
        name: "Release",
        namespace: "acme.delivery-jobs",
        key: "release",
      }),
    ).toEqual({
      name: "",
      namespace:
        "Each namespace segment must start with a letter or underscore and contain only letters, numbers, or underscores.",
      key: "",
    });
  });

  it("reports every invalid identity field together", () => {
    expect(artifactIdentityErrors({ name: "", namespace: "", key: "bad-key" })).toEqual({
      name: "Name is required.",
      namespace: "Namespace is required; use dot-separated identifiers such as acme.delivery.",
      key: "Stable key must start with a letter or underscore and contain only letters, numbers, or underscores.",
    });
  });
});
