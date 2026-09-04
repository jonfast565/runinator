# AGENTS.md

Guidance for `runinator-console`, the REXRAP cell classifier.

## Ownership

This crate decides whether a notebook cell can be evaluated in-process or requires a workflow run.
It owns no evaluator, HTTP client, database, or provider execution: pure cells delegate to the
REXRAP fragment evaluator and effectful cells to the workflow compiler/runtime path.

## Invariants

- Classification is conservative and the workflow fallback is last and unconditional. Treating an
  effectful cell as pure would execute an action without run durability, retry, timeout, or cancel.
- `CELL_SCOPE` is the authored name `params`; `CONTEXT_ROOT` is the evaluator key `input`. They are
  intentionally different. Build evaluation context under `input` even though source spells
  `params`.
- Keep language meaning in the REXRAP crates and execution semantics in engine/runtime.

## Where to Start

- Cell classification and delegation: `src/`.
- Language rules: `../runinator-rexrap/AGENTS.md`.

## Verification

```bash
cargo check -p runinator-console
cargo test -p runinator-console
```

Cover both pure and effectful classification plus `params` resolution.
