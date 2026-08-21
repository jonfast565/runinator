# AGENTS.md

Guidance for agents working in `runinator-broker`.

## Ownership

`runinator-broker` owns concrete broker transports (HTTP/TCP/WS), direct adapters (Kafka and
RabbitMQ), and backend construction. The backend-neutral broker contract, delivery wrappers,
capabilities, errors, and in-memory backend belong to `runinator-broker-core`; this crate
re-exports that surface for binaries that construct a backend.

## Where To Start

- Re-exported public surface and backend factory: `src/lib.rs`, `src/factory.rs`.
- Broker contract, message types, capabilities, errors, and in-memory backend: `../runinator-broker-core/src/`.
- HTTP transport: `src/http/`.
- TCP transport: `src/tcp/`.
- Optional adapters: `src/adapters/`.
- Broker process entry point: `src/bin/main.rs`.
- Transport tests: `tests/http.rs`, `tests/tcp.rs`, adapter-specific tests.

## Boundaries

- Channels are `action`, `control`, `agent`, `result`, `wake`, `ingress`, and fan-out `events`; adding a channel requires every backend and wire transport to be updated together.
- Shared command payloads crossing worker/waker/ws boundaries belong in `runinator-comm`, not broker-local copies.
- The broker should not know about concrete providers, database schema, web handlers, or workflow VM logic.
- Preserve delivery acknowledgement semantics: consumers acknowledge only after processing is complete at the service layer.
- Keep backend behavior aligned across in-memory, HTTP, TCP, Kafka, and RabbitMQ where the feature applies.

## Verification

Use:

```bash
cargo test -p runinator-broker
cargo check -p runinator-broker
```

For channel or wire-shape changes, also check `runinator-ws`, `runinator-waker`, and `runinator-worker`.
