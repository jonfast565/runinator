# AGENTS.md

Guidance for `runinator-sandbox`.

## Ownership

This crate runs an untrusted `ContainerSpec` and reports the outcome. It is shared by `std.code` and
packaged functions and knows nothing about workflows, providers, or the control plane.

## Invariants

- Enforce deadlines in the host. A payload that ignores its own timeout is the reason the boundary
  exists.
- Drain stdout and stderr concurrently on separate threads. A sequential `try_wait` plus
  `wait_with_output` shape deadlocks when either stream fills its pipe.
- Keep Docker argument construction pure in `src/docker/args.rs` so limits and hardening flags can be
  tested without Docker installed.
- Do not add provider/function policy here; callers supply the container specification.

## Where to Start

- Public execution surface: `src/lib.rs`, `src/runner.rs`, `src/spec.rs`.
- Docker arguments and hardening: `src/docker/args.rs`.

## Verification

```bash
cargo check -p runinator-sandbox
cargo test -p runinator-sandbox
```
