// the `runinator-function.json` a package declares itself with, and the publish request it
// describes.
//
// mirrors `runinator-pack::functions::manifest`. the rules here are the ones the backend would
// reject on anyway — they are repeated so the dialog can say what is wrong before it uploads
// several megabytes of archive, not because the client is trusted to enforce them.

import type { FunctionExport, FunctionResourceLimits, FunctionRuntimeSpec } from "./index";

/// the alias a publish moves when the manifest names none.
export const DEFAULT_ALIAS = "latest";

export const MANIFEST_FILE = "runinator-function.json";

/// one export as a manifest declares it, before the server assigns it ids.
export type NewFunctionExport = Pick<FunctionExport, "name" | "handler"> &
  Partial<Pick<FunctionExport, "description" | "input" | "output">> & {
    limits?: FunctionResourceLimits;
  };

export interface FunctionManifest {
  name: string;
  namespace?: string | null;
  description?: string | null;
  runtime: FunctionRuntimeSpec;
  exports: NewFunctionExport[];
  /// the alias to move onto the published version. `null` publishes without moving anything, which
  /// is how a release is staged before promotion.
  alias?: string | null;
  /// extra paths left out of the archive. read by the compiler, not by a publish from a zip that is
  /// already built, so it is carried through untouched.
  exclude?: string[];
}

export interface NewFunctionPackage {
  name: string;
  namespace?: string | null;
  description?: string | null;
}

export interface NewFunctionVersion {
  package: NewFunctionPackage;
  artifact_digest: string;
  /// the manifest kept verbatim, so a republish can be compared against what was published.
  manifest: FunctionManifest;
  runtime: FunctionRuntimeSpec;
  exports: NewFunctionExport[];
  alias?: string | null;
}

/// the publish request a manifest describes, for an archive with the given digest.
export function publishRequest(
  manifest: FunctionManifest,
  artifactDigest: string,
): NewFunctionVersion {
  return {
    // the server stamps the caller's org; a manifest never names one, so a publish cannot land in
    // an org the publisher does not belong to.
    package: {
      name: manifest.name,
      namespace: manifest.namespace ?? null,
      description: manifest.description ?? null,
    },
    artifact_digest: artifactDigest,
    manifest,
    runtime: manifest.runtime,
    exports: manifest.exports,
    alias: manifest.alias === undefined ? DEFAULT_ALIAS : manifest.alias,
  };
}

/// read a manifest out of json text, rejecting anything that would publish something unaddressable
/// or uncallable.
export function parseManifest(text: string): FunctionManifest {
  let parsed: unknown;

  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(`${MANIFEST_FILE} is not valid json`, { cause: error });
  }

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`${MANIFEST_FILE} must be a json object`);
  }

  const manifest = parsed as FunctionManifest;
  validateManifest(manifest);
  return manifest;
}

export function validateManifest(manifest: FunctionManifest): void {
  // the value arrives as parsed json, so nothing about its shape is guaranteed yet; it is read
  // through a partial view until each field has been checked.
  const declared = manifest as Partial<FunctionManifest>;
  validateIdent("package name", declared.name);

  if (declared.namespace) {
    validateIdent("namespace", declared.namespace);
  }

  const runtime: string | undefined = declared.runtime?.runtime;

  if (!runtime?.trim()) {
    throw new Error("runtime must name a runtime");
  }

  if (!Array.isArray(declared.exports) || declared.exports.length === 0) {
    throw new Error("a package must declare at least one export");
  }

  const seen = new Set<string>();

  for (const entry of declared.exports as Partial<NewFunctionExport>[]) {
    validateIdent("export name", entry.name);

    if (!entry.handler?.trim()) {
      throw new Error(`export '${entry.name}' has no handler`);
    }

    if (seen.has(entry.name)) {
      throw new Error(`export '${entry.name}' is declared twice`);
    }

    seen.add(entry.name);
  }

  if (declared.alias) {
    validateIdent("alias", declared.alias);
  }
}

// package, namespace, export, and alias names all become part of a dotted call path
// (`functions.<namespace>.<package>.<export>`), so they have to survive being split on `.`.
function validateIdent(what: string, value: string | undefined): asserts value is string {
  if (!value) {
    throw new Error(`${what} must not be empty`);
  }

  if (value.length > 64) {
    throw new Error(`${what} '${value}' is longer than 64 characters`);
  }

  if (!/^[A-Za-z]/.test(value)) {
    throw new Error(`${what} '${value}' must start with a letter`);
  }

  if (!/^[A-Za-z0-9_-]+$/.test(value)) {
    throw new Error(`${what} '${value}' may only contain letters, digits, '_', and '-'`);
  }
}
