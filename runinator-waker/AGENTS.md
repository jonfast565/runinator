# AGENTS.md

Guidance for `runinator-waker`.

## Ownership

The waker is a small, horizontally scalable broker-only timer relay. It consumes `wake`, waits until
the due instant, and publishes the prebuilt `WsIngressCommand::SettleEffect` on `ingress`.

## Invariants

- Relay the engine-built `EffectResult` verbatim. The waker has no effect vocabulary and makes no
  retry, timeout, timestamp, or settlement decision.
- Do not execute providers, write to a database, call the web service, depend on `runinator-api`, or
  share a worker channel.
- Preserve broker acknowledgement/redelivery behavior and graceful shutdown. Timing must not create
  a second settlement path; the engine's normal result consumer applies the relayed result.
- Shared wake/ingress payloads belong to `runinator-comm`; transport behavior belongs to the broker.

## Where to Start

- Relay loop and scheduling: `src/`.
- Timer construction/semantics: `../runinator-engine/src/infrastructure_effect_host.rs` and
  `runinator-engine/AGENTS.md`.
- Contracts: `../runinator-comm/src/`; broker behavior: `../runinator-broker/AGENTS.md`.

## Verification

```bash
cargo check -p runinator-waker
cargo test -p runinator-waker
```

Wake changes also require engine settlement tests and the affected broker backend checks.
