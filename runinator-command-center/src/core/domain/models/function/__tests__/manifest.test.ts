import { describe, expect, it } from "vitest";
import { parseManifest, publishRequest } from "../manifest";

const source = {
  name: "image-tools",
  namespace: "runinator.examples",
  runtime: { runtime: "python3.13" },
  exports: [{ name: "resize", handler: "src.images.resize" }],
};

describe("function package manifests", () => {
  it("requires and preserves a dotted namespace", () => {
    const manifest = parseManifest(JSON.stringify(source));
    expect(publishRequest(manifest, "sha256:abc").package.namespace).toBe(
      "runinator.examples",
    );
  });

  it("rejects the pre-namespace manifest shape", () => {
    const { namespace: _namespace, ...legacy } = source;
    expect(() => parseManifest(JSON.stringify(legacy))).toThrow("dotted namespace");
  });
});
