# AGENTS.md

Guidance for the complete `runinator-ws*` family, including sibling handler, middleware, and core
crates. Read this file for changes in any of them.

## Ownership

The HTTP surface is layered so dependencies never point back upward:

```text
runinator-ws                    router/server/websocket/openapi assembly, config, binary
  ├── runinator-ws-identity     auth, organizations, billing
  ├── runinator-ws-authoring    definitions, packs, pipelines, credentials, providers, functions, console
  ├── runinator-ws-runtime      runs, effects, triggers, replicas, automation, observability, health
  ├── runinator-ws-middleware   auth, authz, rate limits, overload
  └── runinator-ws-core         models, responses, events, json, openapi vocabulary/examples
```

Choose a handler crate by what the endpoint is about: identity is who may act, authoring is what
can run, and runtime is what is running. Handler crates may depend on core and middleware, not on
each other. `runinator-ws` assembles the surface and hosts `runinator-engine` by default; it must not
duplicate engine, graph-runtime, worker, waker, provider, or database behavior.

## Endpoint Contract

- An endpoint lives in one `handlers/<domain>.rs` file beside its handler function, its
  `routes<T: DatabaseImpl>(...)` registration, and its `DOCS: &[EndpointDoc]` entry.
- Add a new domain module to the owning
  `../runinator-ws-{identity,authoring,runtime}/src/handlers/mod.rs`; do not create another crate for
  a domain or add per-endpoint knowledge to assembly.
- `runinator-ws/src/router.rs` only merges route fragments and middleware.
  `runinator-ws/src/openapi/mod.rs` only concatenates `DOCS` through `DOC_SETS` and enriches the
  result. Do not add individual routes or endpoint docs to either.
- Shared OpenAPI types/constructors and reusable parameters live in
  `runinator-ws-core/src/openapi/docs.rs`; examples live in
  `runinator-ws-core/src/openapi/examples.rs`.
- `Router::merge` rejects duplicate method/path pairs. Path prefix does not decide domain ownership.
- Preserve `runinator-ws/src/lib.rs` historical re-exports for handlers, models, events, auth/authz,
  and engine repository/stability paths used by assembly code and integration tests.

Two source lints protect this layout:

- `src/openapi/route_parity.rs` compares registered and documented routes. Its explicit
  `ROUTER_SOURCES` list must include every handler module with a `routes()` function.
- `store_access_tests.rs` enforces the direct-store allowlist by `<crate>/<file>`.
- Both discover handler crates through `runinator-ws::HANDLER_CRATES`; update it if a handler crate
  is deliberately added or renamed.

## Persistence Boundary

Handlers authorize, validate transport payloads, call engine application services, and map HTTP
responses. SQL and mapping stay in `runinator-database`; orchestration stays in
`runinator-engine/src/repository/`.

A handler may call `db.*` directly only for the closed thin-CRUD allowlist:

- Identity: authentication, organizations/memberships, and billing.
- Authoring: credentials and catalog.
- Runtime: the health readiness probe's database-connectivity check.

Runs, node runs, triggers/firings, dispatches, notifications/policies, pipelines, and replicas go
through the engine even when the current repository function is one delegation. The deciding test
is whether a future version may touch run state, an outbox, the ready-node queue, or a UI event.
Update `store_access_tests.rs` only when deliberately changing this policy.

## Authorization

- Authorization is deny-by-default. Use typed `Action`, `ScopeRef`, fixed roles,
  `require_scope_action`, and `AuthzChecker::require_resource` or a child-to-parent helper.
- `is_platform_admin()` is the only administrative short-circuit. Service keys receive no implicit
  bypass; machine traffic uses explicit `SystemRole` authority.
- Workflows, pipelines, function packages, and console sessions use the generic ownership registry
  and `Permission` ladder. Resolve children to their stored parent before authorization.
- The command center's `/auth/me` and `/authz/catalog` gating is defense in depth, never backend
  enforcement. See `docs/permissions.md`.

## Where to Start

- Assembly and middleware: `src/router.rs`, `src/server.rs`, `src/lib.rs`.
- Handlers: sibling `runinator-ws-{identity,authoring,runtime}/src/handlers/` directories.
- Shared payloads/events/docs: `../runinator-ws-core/src/`.
- Engine application behavior: `../runinator-engine/src/repository/` and its scoped guide.
- Route/store guards: `src/openapi/route_parity.rs`, `src/store_access_tests.rs`.

## Verification

Use the narrowest useful checks, then include dependent crates for shared payloads:

```bash
cargo check -p runinator-ws
cargo test -p runinator-ws
```

Endpoint changes must pass route/OpenAPI parity and store-access tests. Shared database or runtime
contracts also require their owning crate tests and preferably a final workspace test.
The web-service behavior suite intentionally boots a real SQLite store and crosses handler,
engine, VM, and persistence boundaries; keep those integration tests in the assembly crate.
