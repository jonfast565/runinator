# Hierarchical RBAC

Runinator authorizes every request from live database state. Credentials identify a principal;
they do not carry copied administrative authority.

Authority flows downward through `Platform → Organization → Team → Resource/User`. Assignments and
direct grants are additive. Platform roles are `Member < Auditor < Operator < Admin`;
organization and team roles are `Member < Operator < Admin < Owner`. A platform admin is the only
superuser. Machine principals use explicit `Engine`, `Worker`, `Waker`, `Agent`, or `Replica` system
roles and may also have an action ceiling.

The canonical vocabulary lives in `runinator-models/src/rbac.rs` and is returned by
`GET /authz/catalog`. `GET /auth/me` returns the selected scope, current assignments, and effective
actions. Middleware reloads the enabled principal, session, assignments, and API-key restrictions on
every request, so revocation and demotion take effect immediately.

Top-level shareable resources—workflows, pipelines, function packages, and console sessions—have an
authoritative ownership row and generic grants using `View < Run < Edit < Own` (`Edit` includes
`Run`). Runs, workflow effects, continuations, triggers, artifacts, invocations, cells, gates, approvals, and automation
records inherit through their stored parent. Organization and team roles do not expose user-owned
resources without an explicit grant.

Handlers use `require_scope_action` for scoped operations, `AuthzChecker::require_resource` for
top-level resources, and a database-backed child resolver for descendants. `is_platform_admin()` is
the sole administrative short-circuit. Service credentials never bypass these checks; data-plane
routes require an explicit `SystemRole`.

The generic administration surface is:

- `/authz/assignments/{scope_kind}/{scope_id}` for scoped assignments;
- `/authz/resources/{resource_type}/{resource_id}/grants` for generic ACLs;
- `/authz/resources/{resource_type}/{resource_id}/owner` for ownership transfer;
- `/authz/service-accounts` for machine principals;
- `/authz/catalog` and `/auth/me` for clients.

The Command Center consumes backend action strings directly. It hides or disables unavailable
controls, but backend authorization remains authoritative.
