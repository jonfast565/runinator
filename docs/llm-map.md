# LLM Map

Use this map to load the smallest useful part of the repo for a task. The root `AGENTS.md` remains the source of architectural rules; this file is a routing index.

## Runtime Flow

1. `runinator-ws` owns HTTP, the reducer, persistence orchestration, action dispatch outbox publishing, wake publishing, broker ingress consumption, and broker result consumption.
2. `runinator-waker` consumes `wake`, waits until due, then publishes `WsIngressCommand::Drive` on `ingress`.
3. `runinator-worker` consumes `action`, executes providers/plugins, and publishes results on `result`.
4. `runinator-broker` provides backend-neutral channels and transports.
5. `runinator-database` is the only crate that knows concrete persistence details.

## Task Routing

- Change workflow reducer behavior: start in `runinator-ws/src/orchestration/engine.rs`, then the node-specific file in `runinator-ws/src/orchestration/`.
- Change a node transition or retry/timeout rule: `runinator-ws/src/orchestration/transitions.rs`.
- Change runtime context or `$ref` inputs available to the reducer: `runinator-ws/src/orchestration/context.rs`.
- Change workflow validation or graph invariants shared by JSON and WDL: `runinator-workflows/src/validation.rs` and nearby modules.
- Change WDL syntax or compile/decompile behavior: `runinator-wdl/src/wdl.pest`, `parser.rs`, `lower/`, `decompile/`, `format.rs`, and `tests.rs`.
- Change persistence behavior: add to `runinator-database/src/interfaces.rs`, then SQLite and Postgres implementations.
- Change web API behavior: `runinator-ws/src/handlers/`, `router.rs`, and the appropriate `runinator-ws/src/repository/` module.
- Change API client behavior: `runinator-api/src/`.
- Change broker channel payloads: update `runinator-comm` contracts first, then every relevant broker transport/backend and service consumer.
- Change worker execution/provider resolution: `runinator-worker/src/` and provider crates; do not put provider behavior in core runtime crates.
- Add a provider: create a new `runinator-provider-<name>` crate and expose metadata through `Provider::metadata()`.
- Change command-line import or pack behavior: `runinator-ctl/src/`, plus WDL and docs if syntax changes.
- Change desktop UI workflow editing: `runinator-command-center/src/stores/workflows/`, `src/utils/workflows/`, and `src/components/workflow/`.
- Change supervisor/local stack behavior: `runinator-supervisor/src/`, `runinator-supervisor.json`, `scripts/run-local.sh`, and README examples.

## Contract Checklist

When adding or renaming shared fields, inspect:

- `runinator-models`
- `runinator-comm`
- `runinator-database/src/mappers.rs`
- SQLite and Postgres backend implementations
- `runinator-api`
- `runinator-ctl` import paths
- `runinator-command-center/src/types/models.ts` if user-facing

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
- For reducer work, avoid loading all of `runinator-ws`; load `orchestration/engine.rs`, the relevant node module, `transitions.rs`, and `context.rs`.
- For frontend workflow editing, load `stores/workflows/index.ts` for orchestration, `stores/workflows/helpers.ts` for defaults/editor helpers, and `utils/workflows/index.ts` for graph/data transforms.
