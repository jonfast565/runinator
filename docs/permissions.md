# Hierarchical RBAC

Runinator authorizes every request from live database state. Credentials identify a principal;
they do not carry copied administrative authority.

Authority flows downward through `Platform → Organization → Team → Resource/User`. Assignments and
direct grants are additive. Human platform access is `Admin` or absent. Service platform roles are `Member < Auditor < Operator < Admin`;
organization and team roles are `Member < Operator < Admin < Owner`. A platform admin is the only
superuser. Machine principals use explicit `Engine`, `Worker`, `Waker`, `Agent`, or `Replica` system
roles and may also have an action ceiling.

The canonical vocabulary lives in `runinator-models/src/rbac.rs` and is returned by
`GET /authz/catalog`. `GET /auth/me` returns the selected scope, current assignments, and effective
actions. Middleware reloads the enabled principal, session, assignments, and API-key restrictions on
every request, so revocation and demotion take effect immediately.

Platform is an implicit, ID-less scope, never an organization row. Bootstrap provisions the initial
administrator from `RUNINATOR_AUTH_BOOTSTRAP_ADMIN`; only local development configurations supply
`admin:admin`. Human platform administrators can create and refresh sessions without an organization.
Other users require an enabled organization membership. Login returns
an org-less token; the client selects an active organization through `/auth/switch-org` before
performing organization-scoped work. A platform administrator may return to the platform scope
through `/auth/switch-platform`. This prevents a non-admin credential that has lost all organization
membership from retaining a usable session.

New users have no platform assignment. User creation accepts an omitted/null `platform_role` or
`admin`; updates distinguish omission (unchanged), null (remove access), and `admin` (promote).
Multiple human admins are supported, but the last enabled human admin cannot be removed, disabled,
or demoted, even when service administrators exist. The normalized organization slug `platform`
is reserved.

Bootstrap atomically reconciles the legacy `platform` organization: supported resources and their
ownership move to Platform, missing ownership is backfilled, dangling ownership/grants are removed,
and expired ingress-session metadata is deleted. Active ingress state, teams, organization-only
assets, conflicting tenants, or other unsupported references abort with `RUNI513` and roll back
the reconciliation. Platform is never represented by a synthetic organization in API responses.

Top-level shareable resources—workflows, pipelines, function packages, and console sessions—have an
authoritative ownership row and generic grants using `View < Run < Edit < Own` (`Edit` includes
`Run`). Runs, workflow effects, continuations, triggers, artifacts, invocations, cells, gates, approvals, and automation
records inherit through their stored parent. Organization and team roles do not expose user-owned
resources without an explicit grant.

Handlers use `require_scope_action` for scoped operations, `AuthzChecker::require_resource` for
top-level resources, and a database-backed child resolver for descendants. `is_platform_admin()` is
the sole administrative short-circuit. Service credentials never bypass these checks; data-plane
routes require an explicit `SystemRole`.

Execution-profile bundle downloads use `require_system_role(Worker)`, including its platform-admin
override and API-key action ceiling. Every download also requires a consuming run whose stored
snapshot references the profile, matching organization scope, and the current enabled, unexpired
revision. Desktop collection/publication authority alone does not grant bundle download access.

The generic administration surface is:

- `/authz/assignments/{scope_kind}/{scope_id}` for scoped assignments;
- `/authz/resources/{resource_type}/{resource_id}/grants` for generic ACLs;
- `/authz/resources/{resource_type}/{resource_id}/owner` for ownership transfer;
- `/authz/service-accounts` for machine principals;
- `/authz/catalog` and `/auth/me` for clients.

The Command Center consumes backend action strings directly. It hides or disables unavailable
controls, but backend authorization remains authoritative.
