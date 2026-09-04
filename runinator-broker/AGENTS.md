# AGENTS.md

Guidance for `runinator-broker` and sibling `runinator-broker-core`.

## Ownership

`runinator-broker-core` owns the backend-neutral `Broker` trait, channel messages/deliveries,
capabilities, errors, instrumentation, and in-memory backend. It depends on no transport or external
system. Crates that only use `dyn Broker` depend on core.

`runinator-broker` owns backend construction and the HTTP, TCP, WebSocket, Kafka, and RabbitMQ
transports. It re-exports core at historical paths so binaries that construct a backend need only
the transport crate. Backend feature flags belong only on crates that actually contain matching
`cfg(feature)` code; do not forward Kafka/RabbitMQ features through passive consumers.

## Channel Invariants

- `effect`: engine to provider worker; split by `EffectExecutor` so infrastructure effects cannot
  be claimed by provider workers.
- `infrastructure_effect`: engine to its infrastructure effect host.
- `effect_result`: worker/infrastructure host back to engine.
- `control`: web service through engine to worker, target-routed by run.
- `agent`: engine to desktop agent, target-routed by replica.
- `wake`: engine to waker; `ingress`: waker/worker/agent toward engine.
- `events`: web service to every web-service replica; unlike all other competing-consumer channels,
  it is fan-out with one delivery per subscriber/replica.

`effect`, `control`, and `agent` commands carry an `ActionTarget` and are consumed with a matching
`ConsumerProfile`. Backends without native routing must reject/requeue mismatches consistently.
RabbitMQ uses an agent routing key; Kafka event consumers use per-replica groups.

Adding or changing a channel requires aligned behavior across in-memory, HTTP, TCP, WebSocket,
Kafka, RabbitMQ, both wire transports, delivery wrappers, and all relevant services. Shared command
payloads belong in `runinator-comm`, never broker-local copies. Preserve service-level acknowledgement
semantics: a consumer acknowledges only after its required processing is durable.

## Where to Start

- Core contract/backends: `../runinator-broker-core/src/`.
- Factory and re-exports: `src/factory.rs`, `src/lib.rs`.
- Wire transports: `src/http/`, `src/tcp/`.
- Optional backends: `src/adapters/`.
- Transport/integration coverage: `tests/`.

## Verification

```bash
cargo test -p runinator-broker
cargo check -p runinator-broker
```

For a channel/wire change, also check `runinator-ws`, `runinator-engine`, `runinator-waker`, and
`runinator-worker`. A default workspace test does not compile opt-in backends; compile the affected
feature with `--all-targets`.

Kafka and RabbitMQ integration tests are ignored and self-skip when their environment variable is
missing. CI's `broker-backends` job keeps `--nocapture` and fails if it sees the skip line; a green
exit code alone does not prove a broker was exercised. Preserve that guard. Kafka also requires its
topics to exist; use the `Start Kafka` step in `.github/workflows/ci.yml` as the local reference.
