// packaged functions: immutable code published to the platform and invoked as ordinary actions.
// mirrors runinator-models/src/functions.rs; the wire names are the contract.

// a named package. carries no code itself — versions do — so aliases and grants have something
// stable to point at while versions come and go.
export interface FunctionPackage {
  id: string;
  org_id?: string | null;
  namespace?: string | null;
  name: string;
  description?: string | null;
  // the version the default alias resolves to, denormalised so a list needs no join.
  latest_version?: number | null;
  created_at: string;
  updated_at: string;
}

// how a package's code runs.
export interface FunctionRuntimeSpec {
  runtime: string;
  image?: string | null;
  setup_script?: string | null;
}

// what one invocation may consume. every field has a value: an omitted limit means "the default",
// never "unlimited".
export interface FunctionResourceLimits {
  timeout_seconds: number;
  memory_mb: number;
  cpu_millis: number;
  pids: number;
  tmp_mb: number;
  network: boolean;
}

// one immutable release, pinned to exactly one artifact.
export interface FunctionVersion {
  id: string;
  package_id: string;
  version: number;
  artifact_digest: string;
  runtime: FunctionRuntimeSpec;
  published_by?: string | null;
  created_at: string;
}

// one callable entry point.
export interface FunctionExport {
  id: string;
  version_id: string;
  name: string;
  handler: string;
  description?: string | null;
  input?: FunctionParameter[];
  output?: FunctionResult[];
  limits?: FunctionResourceLimits;
}

// a declared input. `ty` rather than `type` because that is the wire name the backend serializes.
export interface FunctionParameter {
  name: string;
  ty: unknown;
  description?: string | null;
  required?: boolean;
}

export interface FunctionResult {
  name: string;
  ty: unknown;
  description?: string | null;
}

// the one mutable part of a published package. moving it changes what *new* calls resolve to and
// nothing else — a compiled workflow recorded the version it was built against.
export interface FunctionAlias {
  id: string;
  package_id: string;
  name: string;
  version_id: string;
  version: number;
  created_at: string;
  updated_at: string;
}

// a package with everything under it.
export interface FunctionPackageDetail extends FunctionPackage {
  versions?: FunctionVersion[];
  aliases?: FunctionAlias[];
  // exports of whatever the default alias points at.
  exports?: FunctionExport[];
}

// one published export, flattened. this is what the catalog lists and what an author calls.
export interface FunctionCatalogEntry {
  package_id: string;
  package_name: string;
  namespace?: string | null;
  version_id: string;
  version: number;
  export_id: string;
  export_name: string;
  artifact_digest: string;
  description?: string | null;
  input?: FunctionParameter[];
  output?: FunctionResult[];
  aliases?: string[];
}

// the stored bytes, addressed by content.
export interface FunctionArtifact {
  digest: string;
  size_bytes: number;
  uri: string;
  media_type: string;
  created_at: string;
}

// the fully qualified package name, `namespace.name` or just `name`.
export function qualifiedPackageName(pkg: {
  namespace?: string | null;
  name: string;
}): string {
  return pkg.namespace ? `${pkg.namespace}.${pkg.name}` : pkg.name;
}

// the dotted call an author writes, e.g. `functions.image-tools.resize`.
export function functionCallPath(entry: FunctionCatalogEntry): string {
  const pkg = entry.namespace
    ? `${entry.namespace}.${entry.package_name}`
    : entry.package_name;
  return `functions.${pkg}.${entry.export_name}`;
}

// the short digest shown in a table. a full sha-256 is 71 characters and tells a reader nothing the
// first few do not.
export function shortDigest(digest: string): string {
  const hex = digest.startsWith("sha256:") ? digest.slice("sha256:".length) : digest;
  return hex.slice(0, 12);
}
