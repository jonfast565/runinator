# AGENTS.md

Guidance for agents working in `runinator-ws`.

## Ownership

`runinator-ws` owns the HTTP API, reducer, repository orchestration, background loops, and broker ingress/result handling. It should depend on `runinator-database::DatabaseImpl` for persistence and shared contracts from `runinator-models`/`runinator-comm`; it should not depend on worker, waker, provider, or plugin internals.

## Where To Start

- HTTP routes and handler wiring: `src/router.rs`, `src/handlers/`.
- Workflow reducer entry point: `src/orchestration/engine.rs`.
- Node-specific reducer behavior: `src/orchestration/action.rs`, `wait.rs`, `basic.rs`, `control_flow.rs`, `approval.rs`, `subflow.rs`.
- Reducer transition/context helpers: `src/orchestration/transitions.rs`, `context.rs`.
- Repository facade: `src/repository/mod.rs`.
- Workflow definitions/import/export: `src/repository/definitions.rs`.
- Ready-node driving, action dispatch publishing, wake publishing, run queries: `src/repository/runs.rs`.
- Debug and pause/resume/cancel behavior: `src/repository/debug.rs`.
- Broker result application and node-run artifacts/logs: `src/repository/node_runs.rs`.

## Boundaries

- Keep SQL and backend-specific persistence in `runinator-database`.
- Keep repository functions thin: validate, orchestrate database calls, publish broker commands, and map web responses.
- Keep command payloads crossing broker boundaries in `runinator-comm`.
- The reducer may enqueue `ActionCommand`s through the durable outbox, but the waker must never publish action commands.
- Do not add direct worker or waker calls from this crate; use broker channels or shared API/client contracts as appropriate.

## Verification

Use the narrowest useful check first:

```bash
cargo check -p runinator-ws
cargo test -p runinator-ws
```

If shared contracts or database behavior changed, also run the affected shared crate tests and prefer a workspace test before handoff.
