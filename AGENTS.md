# AGENTS.md

Repository-wide guidance for Runinator. Keep this file limited to rules that apply across the
workspace. Before changing a subsystem, read its scoped `AGENTS.md`; the routing table below names
family guides that also apply to sibling crates outside the guide's directory.

When adding guidance, put a rule here only if every workspace task needs it. Put ownership,
implementation invariants, and focused verification in the crate that owns them. Prefer a short
rule plus a source-of-truth path over implementation history.

## Development Workflow

### Versioning and Git

- Increment the build number for every new feature. Increment the minor version as well for a
  substantial, multi-phase feature.
- Commit every change and push it directly to `main`.
- Preserve unrelated user changes in a dirty worktree and keep generated/runtime artifacts out of
  commits, especially `build/`, `target/`, and `.runinator-supervisor/`.

### Deployment

Redeploy the cluster after every change except:

- Documentation-only changes: do not deploy.
- UI-only changes: deploy only `runinator-command-center`.
- Changes confined to `runinator-desktop-agent`: do not rebuild or redeploy the cluster unless a
  shared runtime dependency or cluster-owned artifact also changed.

All agent-driven Kubernetes mutations must go through `cargo run -p xtask -- k8s ...`. Never call
`kubectl`, Helm, or `scripts/deploy-k8s.sh` directly for deployment. Mutating `xtask k8s` commands
hold the cluster-backed deployment Lease across build, push, apply, and rollout; do not create a
second deployment path that bypasses that serialization.

### Local Cluster Debugging

Before debugging the cluster with a locally running Command Center or `runinatorctl`, start
`scripts/port-forward-ws.sh` and point the client at that exact forwarded port. The CLI launcher
does not create the port-forward. The development-only bootstrap account is `admin` / `admin`;
production deployments must provide their own credentials.

### Verification Cycle

- Read nearby code before editing and mirror its naming, async style, and error conventions.
- Run compilation, Clippy, and tests at the end of a development cycle rather than after each edit.
- Start with the narrowest relevant check, then use workspace checks for shared contracts.
- Default workspace tests build default features only. Changes to optional broker/provisioner or
  deployed feature sets must compile the affected exact features with `--all-targets`.
- On macOS, run `cargo clean` only after all final compilation, Clippy, and test commands finish.

Typical final Rust checks are:

```bash
cargo fmt --all --check
cargo test -p <crate>
cargo test --workspace
```

Use `cargo check -p <crate>` when the crate has no tests or a full test is unnecessarily slow.
Scoped guides contain required backend, feature, UI, and cross-platform checks.

## Architecture and Task Routing

Runinator is a Rust workspace for authoring, scheduling, and executing workflows through a durable,
resumable state-machine runtime. `docs/architecture.md` explains the system; `docs/llm-map.md` is
the detailed routing index.

| Change area | Owning crate or family | Required scoped guide |
| --- | --- | --- |
| Shared domain and wire models | `runinator-models`, `runinator-comm` | This file |
| Expressions and graph validation | `runinator-compute`, `runinator-workflows` | `runinator-workflows/AGENTS.md` |
| REXRAP syntax, semantics, codegen, and IDE seam | `runinator-rexrap*` | `runinator-rexrap/AGENTS.md` |
| Pack compilation and wire archives | `runinator-pack`, `runinator-pack-wire` | `runinator-pack/AGENTS.md` |
| Continuation interpreter and graph transitions | `runinator-runtime` | `runinator-runtime/AGENTS.md` |
| Durable orchestration and repositories | `runinator-engine`, `runinator-engine-worker` | `runinator-engine/AGENTS.md` |
| HTTP, middleware, and handler crates | `runinator-ws*` | `runinator-ws/AGENTS.md` |
| Persistence contract and SQL backends | `runinator-store`, `runinator-database` | Both local guides |
| Broker contract and transports | `runinator-broker-core`, `runinator-broker` | `runinator-broker/AGENTS.md` |
| Provider execution and desktop worker | `runinator-worker`, `runinator-desktop-agent`, plugins/providers | `runinator-worker/AGENTS.md` |
| Database action provider | `runinator-provider-db` | `runinator-provider-db/AGENTS.md` |
| Timer relay | `runinator-waker` | `runinator-waker/AGENTS.md` |
| Artifact storage | `runinator-blob-core`, `runinator-blob` | `runinator-blob/AGENTS.md` |
| Inbound orchestration adapters | `runinator-adapter-*` | `runinator-adapter-host/AGENTS.md` |
| CLI, native console, MCP, and browser command language | `runinator-ctl*` | `runinator-ctl/AGENTS.md` |
| REXRAP cell classification | `runinator-console` | `runinator-console/AGENTS.md` |
| Command Center UI | `runinator-command-center` | `runinator-command-center/AGENTS.md` |
| Sandboxed container execution | `runinator-sandbox` | `runinator-sandbox/AGENTS.md` |
| Packaged-function execution | `runinator-provider-functions` | `runinator-provider-functions/AGENTS.md` |

Crates without a scoped guide follow this file and the ownership boundaries in
`docs/architecture.md`. If a change requires a dependency from a shared/low-level crate back into a
service crate, stop and redesign the boundary.

## Cross-Cutting Boundaries

- `runinator-models` contains shared domain and wire structs, not services, database details,
  transports, or runtime configuration.
- `runinator-comm` owns broker/discovery contracts. Payloads crossing worker, waker, agent, engine,
  or web-service boundaries must use these shared types end to end; do not create local duplicates.
- `runinator-store` owns persistence traits and plain exchange types. `runinator-database` owns SQL,
  row mapping, migrations, and concrete SQLite/Postgres/MariaDB implementations.
- `runinator-api` owns web-service URL discovery behind locator types. Do not spread raw endpoint
  construction through workers or CLI commands.
- `runinator-runtime` owns graph interpretation through a durable host boundary. It must not gain
  HTTP, concrete broker transports, service hosting, or SQLx.
- `runinator-engine` owns durable orchestration, background loops, and repository application
  behavior. The web service may host it but must not duplicate it.
- `runinator-ws` and its sibling handler crates own HTTP/WebSocket transport, authentication,
  authorization, validation, and response mapping; handlers delegate orchestrated work to the
  engine.
- `runinator-worker` resolves and executes providers. Workers and providers do not schedule work or
  write directly to the database.
- `runinator-waker` is a stateless timer relay. It does not execute providers, decide settlements,
  call the web service, or write to the database.
- `runinator-command-center` is a Tauri/Vue control client. It never hosts a worker or executes
  provider actions; local execution belongs to `runinator-desktop-agent`.
- Inbound adapter polling/webhooks belong to `runinator-adapter-*`; outbound actions belong to
  `runinator-provider-*`. Do not combine the two directions.
- `runinator-platform` owns application paths/process lifecycle and may depend on
  `runinator-observability`; observability must not depend back on platform, application paths, or
  secrets. Keep crypto/plaintext persistence auditable inside `runinator-secrets`.
- Prefer one purpose-named context or request object when an operation needs several related inputs.
  Do not retain a long positional parameter list merely to avoid introducing a small domain type.

## Runtime and Transport Contract

- The engine drives durable continuations in `runinator-runtime`, drains durable effect dispatches,
  and publishes provider work through the broker. The same engine implementation runs embedded in
  the web service or inside `runinator-engine-worker`; never fork those execution paths.
- Provider effects travel engine -> `effect` -> worker -> `effect_result` -> engine. Timed
  infrastructure effects travel engine -> `wake` -> waker -> `ingress` -> engine and then use the
  ordinary effect-result settlement path. Do not add a second retry, timeout, or settlement path.
- `ingress` is the direction toward the engine. API control uses `control`; durable replica
  directives use `agent`. Never add direct worker-to-waker or waker-to-worker channels.
- The authenticated desktop relay permits effect, effect-result, control, and agent operations,
  plus payload-gated ingress publication for `AgentDirectiveResult`. Never expose unrestricted
  ingress publication to an agent principal.
- Broker messages remain serializable and backend-neutral. A channel or shared payload change must
  be implemented across every relevant backend, wire transport, delivery wrapper, producer, and
  consumer.
- Workers acknowledge work only after required durable processing. Worker results, logs, artifacts,
  and node status return as broker events consumed by the engine.
- Artifact bytes live in the object store, never on one replica's filesystem. The engine owns
  artifact storage and database rows; workers upload produced bytes before publishing the event
  that records the artifact.

## Persistence and API Guidance

- Add a persistence operation to the owning `runinator-store` role, then implement one generic
  `SqlStore<B>` body in the matching `runinator-database/src/operations/` module. Do not add domain
  operations directly to `DatabaseImpl`.
- Keep SQLx mapping in `runinator-database`, HTTP response mapping in the relevant handler crate,
  and orchestration behavior in `runinator-engine/src/repository/`.
- Put public payloads in shared model/API crates when multiple binaries or the command center use
  them.

### When a ws handler may call the store directly

A handler may call `db.*` only for the closed thin-CRUD allowlist enforced by
`runinator-ws/src/store_access_tests.rs`: identity authentication/org/billing handlers, authoring
credential/catalog handlers, and the runtime health readiness probe. Anything with orchestration
semantics—including runs, triggers, dispatches, notifications, pipelines, and replicas—goes through
`runinator-engine/src/repository/`, even when the current body is one call. Update the allowlist only
when deliberately changing this policy.

## Shared Contract Changes

When adding or renaming a shared field, inspect every boundary that serializes, persists, or maps it:

- `runinator-models` and `runinator-comm`.
- The owning `runinator-store` role or runtime-store trait.
- `runinator-database` mappers, generic operations, and all three SQL dialects.
- `runinator-runtime` host/interpreter tests when a VM store contract changes.
- `runinator-api`, `runinator-ctl` import/pack paths, and command-center models when user-facing.
- Broker backends and delivery wrappers when the payload crosses a broker channel.

## Authorization

Authorization is deny-by-default and hierarchical; `docs/permissions.md` is the full model.

- Use typed `Action`, `ScopeRef`, platform/organization/team roles, `require_scope_action`, and
  `AuthzChecker::require_resource`. Never add an admin boolean, principal-kind bypass, or second
  capability catalog.
- `is_platform_admin()` is the only administrative short-circuit. Machine traffic uses an explicit,
  non-assignable `SystemRole`, never service-key status alone.
- Owned resources use the generic ownership registry and `Permission` ladder. Resolve a child to
  its stored parent before authorization.
- Command-center gating is defense in depth and never replaces backend enforcement.

## Configuration, Errors, and Async

- For runtime option changes, inspect the owning `config.rs`/`cli.rs`,
  `runinator-supervisor.json`, relevant READMEs, Kubernetes manifests/overlays, service Dockerfiles,
  and `xtask` build/deploy plumbing.
- Preserve the local supervisor flow: `cargo build --workspace`, then
  `cargo run -p runinator-supervisor -- start|status|stop`.
- `tools/keychain-export` is a macOS execution-profile collector approved by complete configuration
  digest; never couple it to Kubernetes sync or provider mounts. Scalar secrets continue through
  `CredentialStore`, `SecretCipher`, and settings `secret://` references.
- Prefer `SendableError` and structured `RuntimeError` where already established. Do not use
  `unwrap`/`expect` in runtime paths unless the process truly cannot continue and nearby code uses
  that convention.
- Every Rust crate error uses a stable numbered `ErrorDescriptor` from the owning crate/family
  dictionary. Add the next code in that range; do not hand-roll one-off code strings. Keep
  `thiserror` literals synchronized with their dictionary entries. The command-center TypeScript
  client is outside this catalog.
- `REXRAP`, `WORKFLOW`, `BROKER`, and `BLOB` dictionaries are owned by their lowest shared/core
  crate and re-exported upward; do not create per-wrapper copies.
- Preserve existing domain prefixes and numeric ranges. `RUNI` is only the partitioned fallback for
  runtime crates without their own vocabulary; domain crates use their domain prefix.
- Library crates expose `pub mod errors;` so binaries can name descriptors. A binary-owned
  dictionary may allow dead code where public descriptors are intentionally unused locally.
- Keep blocking provider/plugin work behind `spawn_blocking` or equivalent isolation, preserve
  graceful shutdown, and never hold a lock across `.await`.

## Coding and Change Hygiene

- Favor guard clauses over deep nesting and traits for behavior with multiple implementations.
- Keep comments lower case, single-line, and punctuated where practical. Keep public RustDoc short,
  dense, and dispassionate.
- Split libraries into focused modules. Prefer one primary struct or trait per file; treat 500 lines
  as a review threshold, not a mechanical limit.
- Keep unit tests out of production files. Pair `module.rs` with `module_tests.rs`, or use a focused
  `<subject>_tests/` directory whose child modules start with `use super::*;` and a `//!` subject
  description. Do not add generic unit-test `tests.rs` files/directories; Cargo integration tests
  remain in top-level `tests/`.
- Keep changes scoped to the crate that owns the behavior. Avoid broad refactors and new workspace
  dependencies for small conveniences; do not move shared structs between crates casually.
- Update documentation and configuration examples in the same change as behavior.

Before adding code, confirm that the owning crate is correct, dependency direction still points
toward shared contracts, serializers/mappers/clients/config are updated, and the local supervisor
stack can still run.
