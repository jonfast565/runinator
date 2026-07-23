# LLM Map

Use this map to load the smallest useful part of the repo for a task. The root `AGENTS.md` remains the source of architectural rules; this file is a routing index.

## Runtime Flow

1. `runinator-ws` owns HTTP/WebSocket transport and auth, and hosts `runinator-engine` by default.
2. `runinator-engine` owns persistence orchestration and the loops for ingress/results, triggers, wakes, action dispatch, and reconciliation; `runinator-background-worker` can host the same engine out of process.
3. `runinator-reducer` owns state-machine transitions and node-kind behavior.
4. `runinator-waker` consumes `wake`, waits until due, then publishes `WsIngressCommand::Drive` on `ingress`.
5. `runinator-worker` consumes `action`, executes providers/plugins, and publishes results on `result`; `runinator-desktop-agent` hosts that runtime as an exclusive desktop worker.
6. `runinator-broker` provides backend-neutral channels and transports, and `runinator-database` owns concrete persistence.

## Task Routing

- Change workflow reducer behavior: start in `runinator-reducer/src/orchestration/engine.rs`, then the node-specific file in `runinator-reducer/src/orchestration/`.
- Change a node transition or retry/timeout rule: `runinator-reducer/src/orchestration/transitions.rs`.
- Change runtime context or `$ref` inputs available to the reducer: `runinator-reducer/src/orchestration/context.rs`.
- Change workflow validation or graph invariants shared by JSON and WDL: `runinator-workflows/src/validation.rs` and nearby modules.
- Change WDL syntax or compile/decompile behavior: `runinator-wdl/src/wdl.pest`, `parser.rs`, `lower/`, `decompile/`, `format.rs`, and `tests.rs`.
- Change persistence behavior: add to `runinator-database/src/interfaces.rs`, then SQLite and Postgres implementations.
- Change durable orchestration/repository behavior: `runinator-engine/src/repository/` and its background loops.
- Change web API behavior: `runinator-ws/src/handlers/` and `runinator-ws/src/router.rs`.
- Change API client behavior: `runinator-api/src/`.
- Change broker channel payloads: update `runinator-comm` contracts first, then every relevant broker transport/backend and service consumer.
- Change worker execution/provider resolution: `runinator-worker/src/` and provider crates; do not put provider behavior in core runtime crates.
- Change desktop-agent lifecycle, tray UI, sandbox, or desktop routing: `runinator-desktop-agent/src/`; keep reusable worker-loop behavior in `runinator-worker` and never add it to the command center.
- Add a provider: create a new `runinator-provider-<name>` crate and expose metadata through `Provider::metadata()`.
- Change command-line import or pack behavior: `runinator-ctl/src/`, plus WDL and docs if syntax changes.
- Change desktop UI workflow editing: `runinator-command-center/src/core/services/workflows/`, `src/core/workflow/`, `src/ui/adapters/pinia/workflows/`, and `src/ui/components/workflow/`.
- Change supervisor/local stack behavior: `runinator-supervisor/src/`, `runinator-supervisor.json`, `scripts/run-local.sh`, and README examples.

## Contract Checklist

When adding or renaming shared fields, inspect:

- `runinator-models`
- `runinator-comm`
- `runinator-database/src/mappers.rs`
- SQLite and Postgres backend implementations
- `runinator-api`
- `runinator-ctl` import paths
- `runinator-command-center/src/core/domain/models/` if user-facing

## Verification Shortcuts

- Web service only: `cargo check -p runinator-ws`.
- Database behavior: `cargo test -p runinator-database`.
- WDL behavior: `cargo test -p runinator-wdl`.
- Broker behavior: `cargo test -p runinator-broker`.
- Command center: `pnpm --dir runinator-command-center test -- --run` and `pnpm --dir runinator-command-center build`.
- Shared contracts: prefer `cargo test --workspace` after narrow checks.

## Loading Hints

- Start with the local `AGENTS.md` inside a crate before reading implementation files.
- Prefer module facades (`mod.rs`, `lib.rs`, store `index.ts`) to learn the layout, then open only the behavior-specific file.
- For reducer work, avoid loading all of `runinator-engine` or `runinator-ws`; load `runinator-reducer/src/orchestration/engine.rs`, the relevant node module, `transitions.rs`, and `context.rs`.
- For frontend workflow editing, load `core/services/workflows/index.ts` and its focused service modules, `core/workflow/` for graph/data transforms, and `ui/adapters/pinia/workflows/index.ts` for the presentation adapter.
