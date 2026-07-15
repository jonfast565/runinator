# Permission Model

Runinator's authorization has two independent axes. This document is the source of truth for the
first; the code enforces both.

1. **Capabilities** — named, global/organization-scoped privileges (this document). The catalog lives
   in `runinator-models/src/capabilities.rs` (`Capability`), is mirrored to the command center in
   `src/core/domain/models/auth/capability.ts`, and is resolved per-request by
   `runinator-ws/src/authz.rs::capabilities_for`.
2. **Resource grants** — a per-resource ladder (`View < Run < Edit < Own`) on individual workflows and
   pipelines, held by a user or a team. Defined by `Permission` in `runinator-models/src/auth.rs` and
   enforced by `authz::require_workflow` / `require_pipeline` (and the `require_*_workflow` helpers for
   sub-resources). Not enumerated here; see those functions.

## Who holds a capability

Capabilities are resolved from the caller's platform-admin flag and their role in the **active
organization** (`capabilities_for`):

- **Platform admin** (`is_admin`, including the synthetic admin used when auth is disabled) — holds
  **every** capability.
- **Organization admin/owner** (active-org role ≥ `admin`) — holds the organization capabilities only.
- **Organization member** / signed-out — holds none.

When auth is disabled (`RUNINATOR_AUTH_ENABLED=false`, the default), every request runs as the
synthetic platform admin, so nothing is gated.

## Catalog

### Platform capabilities (platform admin only)

| Capability | Grants | Enforced at |
| --- | --- | --- |
| `users:manage` | create/update/delete user accounts | `handlers/auth.rs` (users) |
| `teams:manage` | create/update/delete teams and membership | `handlers/auth.rs` (teams) |
| `apikeys:manage` | administer api keys beyond one's own (service keys, others', rotate/revoke) | `handlers/auth.rs` (api keys) |
| `secrets:read` | read decrypted settings/secrets | `handlers/credentials.rs` (get/list) |
| `secrets:write` | create/update/delete/import/re-encrypt settings/secrets | `handlers/credentials.rs` (mutations) |
| `catalog:manage` | upsert catalog metadata items | `handlers/catalog.rs` |
| `audit:read` | read the audit log | `handlers/observability.rs` |
| `deadletters:read` | read the dead-letter queue | `handlers/observability.rs` |
| `nodes:scale` | scale/stop platform worker nodes | `handlers/provisioning.rs` |
| `workflows:import` | import workflow bundles | `handlers/workflows.rs` |
| `orgs:manage` | platform-wide org administration (list all) | `handlers/orgs.rs` |
| `billing:manage` | set organization billing quotas | `handlers/billing.rs` |
| `settings:manage` | manage platform/admin settings | command center admin settings |

### Organization capabilities (active-org admin, or platform admin)

| Capability | Grants | Enforced at |
| --- | --- | --- |
| `org:members:manage` | manage membership and roles in the active org | `handlers/orgs.rs` (`require_org_admin`) |
| `org:nodes:scale` | scale worker nodes within the active org | `handlers/billing.rs` (`require_org_admin`) |

The organization endpoints keep `require_org_admin(ctx, org_id)`, which also checks the caller is an
admin of the *target* org. The org capabilities above are the command center's signal for the same
rule (they are derived from the active-org role) and must not be applied to platform-global handlers.

## Enforcement

- **Backend is authoritative.** Handlers gate with `authz::require_capability(&ctx, Capability::X)` for
  platform capabilities, `require_org_admin` for org-scoped resources, and `require_workflow` /
  `require_pipeline` for resource grants. `GET /auth/me` returns the caller's resolved capability set
  (`capabilities`), and `/auth/login` embeds it in the token response.
- **Command center gates against `/auth/me`.** Capabilities are stored on the auth principal
  (`core/services/auth.ts`), exposed through `useCapabilitiesStore` / the `useCan()` composable, and
  refreshed on login, refresh, and org switch (`auth.reloadMe()`). The ui **hides** navigation tabs and
  admin panels the caller lacks (`nav-config.ts` `requires`), and **disables** individual action
  controls. This is defense-in-depth and UX; it never replaces backend enforcement.

## Extending the model

Add a new privileged action as a `Capability` variant (backend + the mirrored TS union), map it in
`capabilities_for`, gate the handler with `require_capability`, and document it in the table above.
Prefer a named capability over a new bare `require_admin` gate so both the backend and the ui reference
one dictionary.
