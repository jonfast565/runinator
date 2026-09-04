# LLM Map

Use this map to load the smallest useful part of the repo for a task. The root `AGENTS.md` owns
repository-wide rules and points to scoped `AGENTS.md` files that own subsystem invariants. A family
guide named by the root applies to its listed sibling crates even when normal directory inheritance
does not load it automatically. This file is a routing index, not a second rule set.

## Runtime Flow

1. `runinator-ws` owns HTTP/WebSocket transport and auth, and hosts `runinator-engine` by default.
2. `runinator-engine` owns persistence orchestration: workflow VM/effect loops, effect-result handling, triggers, agent directives, and maintenance. `runinator-ws` hosts it by default; `runinator-engine-worker` is the optional out-of-process host for independently scaled engine replicas. It does not execute providers.
3. `runinator-runtime` owns the host-free bytecode interpreter, durable host boundary, continuation transitions, and instruction behavior.
4. `runinator-waker` consumes `wake`, waits until due, then relays its prebuilt `WsIngressCommand::SettleEffect` on `ingress`.
5. `runinator-worker` consumes provider effects, executes providers/plugins, and publishes effect results; `runinator-desktop-agent` hosts that runtime as an exclusive desktop worker.
6. `runinator-broker` provides backend-neutral channels and transports, and `runinator-database` owns concrete persistence.

## Task Routing

- Change VM instruction or continuation behavior: start in `runinator-runtime/src/workflow_vm.rs`; durable boundary application is in `runinator-runtime/src/workflow_vm_host.rs`.
- Change effect retry or deadline behavior: `runinator-engine/src/effect_retry.rs`, `runinator-engine/src/effect_deadline.rs`, and the ordinary effect-result path.
- Change runtime locals or `$ref` inputs exposed by the interpreter: `local_context` and the relevant instruction handling in `runinator-runtime/src/workflow_vm.rs`.
- Change workflow validation or graph invariants shared by JSON and REXRAP: `runinator-workflows/src/validation.rs` and nearby modules.
- Add or change a node kind's authoring behavior (palette entry, graph role, parameter targets, output type): its file in `runinator-workflows/src/node_kinds/<category>/`, plus an arm in `spec_for`.
- Change REXRAP syntax or compile/decompile behavior: start in `runinator-rexrap-syntax` (`rexrap.pest`, parser, formatter), then the relevant `runinator-rexrap-sema` or `runinator-rexrap-codegen` module; use the facade crate's cross-stage tests.
- Change editor completion or hover: `runinator-rexrap-ide/src/`; if it needs something new from the language core, add it to `runinator-rexrap/src/analysis.rs`.
- Change persistence behavior: add to the owning role trait in `runinator-store/src/roles/`, then the matching file in `runinator-database/src/operations/`.
- Change durable orchestration/repository behavior: `runinator-engine/src/repository/`, the VM/effect loops, and their focused tests.
- Change correlated pipeline orchestration, ingress intents, phase policies, or workspace leases:
  `runinator-models/src/orchestration.rs`, `runinator-engine/src/services/{orchestration_operations,pipeline_ingress,pipeline_operations}.rs`, and the matching store role/database operations.
- Change inbound adapter behavior: the shared ABI in `runinator-adapter-contract`, adapter loading and built-ins in `runinator-adapter-host`, the HTTP client in `runinator-adapter-client`, and the authoring/engine callers. Keep inbound adapters separate from outbound `runinator-provider-*` crates.
- Change web API behavior: use the handler's file in `runinator-ws-{identity,authoring,runtime}/src/handlers/` (authoring = what can run, runtime = what is running, identity = who may do either); `runinator-ws/src/router.rs` only merges them. Shared wire types and the response envelope are in `runinator-ws-core`; auth/authz/rate-limit/overload are in `runinator-ws-middleware`. A handler reaches persistence through `runinator-engine/src/repository/`; only the allowlist in `runinator-ws/src/store_access_tests.rs` calls the store directly.
- Change API client behavior: `runinator-api/src/`.
- Change broker channel payloads: update `runinator-comm` contracts first, then every relevant broker transport/backend and service consumer.
- Change worker execution/provider resolution: `runinator-worker/src/` and provider crates; do not put provider behavior in core runtime crates.
- Change desktop-agent lifecycle, tray UI, sandbox, or desktop routing: `runinator-desktop-agent/src/`; keep reusable worker-loop behavior in `runinator-worker` and never add it to the command center.
- Add a provider: create a new `runinator-provider-<name>` crate and expose metadata through `Provider::metadata()`.
- Change command-line import or pack behavior: `runinator-ctl/src/` and `runinator-pack/AGENTS.md`, plus REXRAP and docs if syntax changes.
- Change desktop UI workflow editing: `runinator-command-center/src/core/services/workflows/`, `runinator-command-center/src/core/workflow/`, `runinator-command-center/src/ui/adapters/pinia/workflows/`, and `runinator-command-center/src/ui/components/workflow/`.
- Change supervisor/local stack behavior: `runinator-supervisor/src/`, `runinator-supervisor.json`, `scripts/run-local.sh`, and README examples.

## Contract Checklist

When adding or renaming shared fields, inspect:

- `runinator-models`
- `runinator-comm`
- `runinator-database/src/mappers.rs`
- SQLite, Postgres, and MariaDB backend implementations
- `runinator-db-cli` when backend selection or connection handling changes
- `runinator-api`
- `runinator-ctl` import paths
- `runinator-command-center/src/core/domain/models/` if user-facing

## Verification Shortcuts

- Web service only: `cargo check -p runinator-ws`.
- Database behavior: `cargo test -p runinator-database`.
- REXRAP behavior: `cargo test -p runinator-rexrap`; editor completion/hover: `cargo test -p runinator-rexrap-ide`.
- Broker behavior: `cargo test -p runinator-broker`.
- Command center: `pnpm --dir runinator-command-center test -- --run` and `pnpm --dir runinator-command-center build`.
- Shared contracts: prefer `cargo test --workspace` after narrow checks.

## Loading Hints

- Start with the root `AGENTS.md`, then read the scoped or family guide named by its routing table
  before opening implementation files.
- A crate without a local file may be covered by a sibling family guide; for example,
  `runinator-ws/AGENTS.md`, `runinator-rexrap/AGENTS.md`, and `runinator-broker/AGENTS.md` explicitly
  cover their sibling crates.
- Prefer module facades (`mod.rs`, `lib.rs`, store `index.ts`) to learn the layout, then open only the behavior-specific file.
- For VM work, avoid loading all of `runinator-engine` or `runinator-ws`; start with
  `runinator-runtime/src/workflow_vm.rs`, `runinator-runtime/src/workflow_vm_host.rs`, and the
  relevant store-contract operation.
- For frontend workflow editing, load
  `runinator-command-center/src/core/services/workflows/index.ts` and its focused service modules,
  `runinator-command-center/src/core/workflow/` for graph/data transforms, and
  `runinator-command-center/src/ui/adapters/pinia/workflows/index.ts` for the presentation adapter.
