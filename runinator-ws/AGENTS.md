# AGENTS.md

Guidance for agents working in `runinator-ws`.

## Ownership

`runinator-ws` owns HTTP/WebSocket transport, authentication/authorization, discovery, and API response mapping. Durable orchestration and background loops live in `runinator-engine`; state-machine transitions live in `runinator-reducer`. The web service hosts the engine by default but must not duplicate its implementation. It should not depend on worker, waker, provider, or plugin internals.

## Where To Start

- HTTP routes and handler wiring: `src/router.rs`, `src/handlers/`.
- Engine startup/hosting: `src/server.rs`; shared engine implementation: `../runinator-engine/src/`.
- Workflow reducer and node behavior: `../runinator-reducer/src/orchestration/`.
- API handlers: `src/handlers/`; handlers call the engine repository facade re-exported in `src/lib.rs`.
- Workflow definitions/import/export: `../runinator-engine/src/repository/definitions.rs`.
- Ready-node driving, action dispatch publishing, wake publishing, and run queries: `../runinator-engine/src/repository/runs.rs` plus `../runinator-engine/src/loops.rs`.
- Debug and pause/resume/cancel behavior: `../runinator-engine/src/repository/debug.rs`.
- Broker result application and node-run artifacts/logs: `../runinator-engine/src/result_consumer.rs` and `../runinator-engine/src/repository/node_runs.rs`.

## Boundaries

- Keep SQL and backend-specific persistence in `runinator-database`.
- Keep HTTP handlers thin: authorize, validate transport payloads, call `runinator-engine`, and map web responses.
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
