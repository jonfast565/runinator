# AGENTS.md

Guidance for agents working in `runinator-broker`.

## Ownership

`runinator-broker` owns broker traits, message/delivery wrappers, in-memory transport, HTTP/TCP broker transports, and optional direct adapters such as Kafka and RabbitMQ. Broker messages must remain serializable and backend-neutral.

## Where To Start

- Broker trait and message types: `src/lib.rs`, `src/types.rs`.
- Capabilities and errors: `src/capabilities.rs`, `src/errors.rs`.
- In-memory backend: `src/in_memory.rs`.
- HTTP transport: `src/http/`.
- TCP transport: `src/tcp/`.
- Optional adapters: `src/adapters/`.
- Broker process entry point: `src/bin/main.rs`.
- Transport tests: `tests/http.rs`, `tests/tcp.rs`, adapter-specific tests.

## Boundaries

- Channels are `action`, `control`, `result`, `wake`, and `ingress`; adding a channel requires every backend and wire transport to be updated together.
- Shared command payloads crossing worker/waker/ws boundaries belong in `runinator-comm`, not broker-local copies.
- The broker should not know about concrete providers, database schema, web handlers, or workflow reducer logic.
- Preserve delivery acknowledgement semantics: consumers acknowledge only after processing is complete at the service layer.
- Keep backend behavior aligned across in-memory, HTTP, TCP, Kafka, and RabbitMQ where the feature applies.

## Verification

Use:

```bash
cargo test -p runinator-broker
cargo check -p runinator-broker
```

For channel or wire-shape changes, also check `runinator-ws`, `runinator-waker`, and `runinator-worker`.
