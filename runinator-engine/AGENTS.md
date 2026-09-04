# AGENTS.md

Guidance for `runinator-engine` and the `runinator-engine-worker` host.

## Ownership

`runinator-engine` owns durable orchestration around the host-free interpreter: repository
application behavior, VM driving, triggers, effect dispatch/results, ingress, agent directives,
and maintenance. It can run inside `runinator-ws` or `runinator-engine-worker`; both hosts use the
same library and must not fork execution paths.

The engine depends on store, broker, blob, and adapter contracts. It does not implement HTTP
response mapping, concrete SQL/backends, provider actions, or graph semantics owned by
`runinator-runtime`.

## Effect and Ingress Invariants

- Node retry applies to an effect, not its parked continuation. `effect_retry.rs` re-arms a
  retryable terminal through `retry_workflow_effect`; the continuation remains waiting while
  `attempt` and the delayed dispatch's `available_at` change. Do not add retries in the worker or VM.
- A known-future infrastructure completion becomes a `WakeCommand` carrying the final
  `EffectResult`. Already-due work settles inline; dedupe is `effect_id:attempt`; the result is
  timestamped at `due_at`. The waker relays it through ingress and the ordinary result consumer
  settles it. Polling effects such as gates, `await_run`, child runs, mutexes, and barriers remain
  engine tasks rather than timer wakes.
- Provider timeouts have two enforcers: the worker's primary timeout and an engine deadline wake
  `DEADLINE_GRACE_SECONDS` later. Both use `DEFAULT_ACTION_TIMEOUT_SECONDS`. Arm the backstop only
  after effect publication so wake failure cannot suppress dispatch; a losing deadline settle is an
  exact transactional no-op.
- `run_ingress_consumer` is the sole engine-facing ingress consumer. It accepts timer settlements,
  worker control requests, and agent directive results. Ack an impossible unknown-directive reply
  rather than requeueing it forever.
- Retry, interrupt handling, run events, and terminal bookkeeping remain in the ordinary
  effect-result path. Never introduce a timer-only or transport-specific settlement path.

## Repository and Artifact Boundaries

- Repository modules coordinate durable behavior through focused store traits. SQL stays in
  `runinator-database`; handler response mapping stays in the web-service family.
- Anything that may touch run state, dispatch/ready-node outboxes, or UI events is engine behavior,
  even if its current repository function is a one-line delegation.
- `artifact_storage` is the only engine module that reads/writes artifact bytes. Persist `blob://`
  references after storing content through `BlobStore`. `POST /artifacts/content` stores bytes but
  no artifact row; the result-event path records the row exactly once.
- Runtime components announce fleet state through ingress. Web-service replicas self-register
  because they own the control-plane store; workers, wakers, and agents do not write replica rows.

## Where to Start

- Engine loops and hosting: `src/engine.rs`, `src/effect_consumer.rs`, `src/ingress_consumer.rs`.
- Effect policy: `src/effect_retry.rs`, `src/effect_deadline.rs`,
  `src/infrastructure_effect_host.rs`.
- Durable application behavior: `src/repository/` and focused services.
- Runtime boundary: `../runinator-runtime/src/workflow_vm.rs`,
  `../runinator-runtime/src/workflow_vm_host.rs`, and its scoped guide.
- Artifact bytes: `src/artifact_storage.rs`.

## Verification

Run focused engine/repository tests, then shared runtime/store tests when their contracts change:

```bash
cargo check -p runinator-engine
cargo test -p runinator-engine
```

Timer, timeout, retry, or settlement changes need race/duplicate-attempt coverage and database
dialect parity for the transactional losing-race behavior.
