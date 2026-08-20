# AGENTS.md

Guidance for agents working in `runinator-ws`.

## Ownership

`runinator-ws` owns HTTP/WebSocket transport, authentication/authorization, discovery, and API response mapping. Background orchestration loops live in `runinator-engine`; the continuation-driven graph interpreter and durable host boundary live in `runinator-runtime`. The web service hosts the engine by default but must not duplicate either implementation. It should not depend on worker, waker, provider, or plugin internals.

This crate is the **assembly** layer. The surface itself lives in five sibling crates —
`runinator-ws-core` (wire types, responses, events, openapi vocabulary), `runinator-ws-middleware`
(auth, authz, rate limiting, overload), and `runinator-ws-{identity,authoring,runtime}` (the handler
modules). See the root `AGENTS.md` section "The web service crates" for the layering and for which
crate a new endpoint belongs in.

## Where To Start

- Route merging and the middleware stack: `src/router.rs`.
- Handlers: `../runinator-ws-{identity,authoring,runtime}/src/handlers/`; they call the engine
  repository facade directly, and `src/lib.rs` re-exports them at `crate::handlers::<domain>` for the
  openapi `paths(...)` table and the test suite.
- Engine startup/hosting: `src/server.rs`; shared engine implementation: `../runinator-engine/src/`.
- Graph interpreter and host boundary: `../runinator-runtime/src/{machine,host}.rs`; node behavior: `../runinator-runtime/src/orchestration/`.
- The two source lints over the merged surface: `src/openapi/route_parity.rs` and
  `src/store_access_tests.rs`; both read `HANDLER_CRATES` in `src/lib.rs`.
- Workflow definitions/import/export: `../runinator-engine/src/repository/definitions.rs`.
- Ready-node driving, action dispatch publishing, wake publishing, and run queries: `../runinator-engine/src/repository/runs.rs` plus `../runinator-engine/src/loops.rs`.
- Debug and pause/resume/cancel behavior: `../runinator-engine/src/repository/debug.rs`.
- Broker result application and node-run artifacts/logs: `../runinator-engine/src/result_consumer.rs` and `../runinator-engine/src/repository/node_runs.rs`.

## Boundaries

- Keep SQL and backend-specific persistence in `runinator-database`.
- Keep HTTP handlers thin: authorize, validate transport payloads, call `runinator-engine`, and map web responses.
- Keep command payloads crossing broker boundaries in `runinator-comm`.
- The graph runtime may enqueue `ActionCommand`s through the durable outbox, but the waker must never publish action commands.
- Do not add direct worker or waker calls from this crate; use broker channels or shared API/client contracts as appropriate.

## Verification

Use the narrowest useful check first:

```bash
cargo check -p runinator-ws
cargo test -p runinator-ws
```

If shared contracts or database behavior changed, also run the affected shared crate tests and prefer a workspace test before handoff.
