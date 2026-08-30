# Advanced engine test suite

This pack is a set of executable integration probes for the workflow VM and pipeline orchestrator.
It intentionally uses only the built-in console provider so it can run on a local stack without
third-party credentials.

The scenarios cover:

- `Advanced VM Fanout`: map concurrency, explicit parallel joins, first-success races, durable
  timers, locked deadlines, mutex acquire/release, and workflow result persistence.
- `Advanced Retry Recovery`: brokered action retry/backoff, idempotency keys, and result
  persistence.
- `Advanced Saga Probe`: try/catch/finally and compensation unwinding.
- `Advanced Mapped Fanin`: concurrent pipeline entry members, an `all` join, cross-member result
  mapping, pipeline concurrency queuing, and downstream validation.
- `Advanced Failure Continuation`: a deliberately failing member, `silently_continue`, a
  completion-selecting edge, and successful cleanup after the expected failure.

The ignored `advanced_engine_pack_exercises_runtime_and_pipelines` test in `runinator-e2e` imports
the pack and verifies retry recovery and both pipeline scenarios against a real local
broker/web-service/waker/worker stack.

`Advanced VM Fanout` is also a regression probe for the durable continuation host. As of build
`0.5.550`, its branches execute and arrive successfully, but the run remains parked because joined
fork cursors are persisted without being reconciled into their coordinator. Keep this scenario in
the pack: a successful terminal run is the acceptance criterion for the engine join fix.

Provider `async`/`await` is intentionally not folded into the fanout probe. Lowering currently
targets a synthetic `task_run_id` node output that the continuation VM does not materialize; that
is a separate engine/codegen defect and should get its own regression when its runtime contract is
defined.

`Advanced Saga Probe` exposes the same missing node-output state plus a structured-unwind gap. A
failed body can enter `catch`, but successful graph edges bypass the try-frame transition that
would run `finally`; a later failure in `catch` can enter `finally` without unwinding registered
compensations. The acceptance result is exactly `forward`, `compensated`, `finally` in `saga_log`
and a terminal success after the catch verifies the compensation.
