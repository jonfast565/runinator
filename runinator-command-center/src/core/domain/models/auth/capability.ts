// the named capability catalog, mirroring runinator-models/src/capabilities.rs. the wire strings are
// the contract with the backend; `/auth/me` returns the caller's set and the ui gates against it.
// per-resource grants (see PermissionLevel) are a separate, resource-scoped axis.

export type Capability =
  // platform capabilities (platform admin only)
  | "users:manage"
  | "teams:manage"
  | "apikeys:manage"
  | "secrets:read"
  | "secrets:write"
  | "catalog:manage"
  | "audit:read"
  | "deadletters:read"
  | "nodes:scale"
  | "workflows:import"
  | "orgs:manage"
  | "billing:manage"
  | "settings:manage"
  // organization capabilities (admin of the caller's active org, or platform admin)
  | "org:members:manage"
  | "org:nodes:scale";

// every capability, matching Capability::ALL on the backend.
export const ALL_CAPABILITIES: readonly Capability[] = [
  "users:manage",
  "teams:manage",
  "apikeys:manage",
  "secrets:read",
  "secrets:write",
  "catalog:manage",
  "audit:read",
  "deadletters:read",
  "nodes:scale",
  "workflows:import",
  "orgs:manage",
  "billing:manage",
  "settings:manage",
  "org:members:manage",
  "org:nodes:scale",
];
