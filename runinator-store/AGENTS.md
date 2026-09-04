# AGENTS.md

Guidance for `runinator-store`, the persistence contract.

## Ownership

This crate contains traits and plain types exchanged with persistence, with no SQLx or backend.
Concrete SQL, mapping, migrations, and dialect behavior belong to `runinator-database`.

## Contract Shape

- `src/roles/` owns one trait per domain. Add an operation to the role that owns the data; do not add
  domain operations directly to `DatabaseImpl`.
- `DatabaseImpl` composes role traits, `RuntimeStore`, and `PackTransactionStore`; it keeps only
  initialization behavior of its own.
- `RuntimeStore` is the narrow use-case contract called by the engine's store-backed runtime host.
  Keep it small enough for the in-memory runtime fake rather than making it a database facade.
- `WorkflowVmStore` owns persistence required by the continuation VM: modules, continuations,
  effects, dispatches, receipts, and journal state.
- Bound callers on the narrowest roles they use. Use `runinator_store::prelude::*` when importing
  several role traits would add noise; do not broaden a caller to `DatabaseImpl` for convenience.
- Shared domain/wire structs remain in `runinator-models`. Store-only exchange types may live here,
  but never duplicate public models just to fit a backend.

Contract changes must be implemented by `SqlStore<B>`, the runtime test fake when applicable, and
every other implementation. Check API/CLI/UI serialization consumers when a field is shared.

## Where to Start

- Role traits: `src/roles/`.
- Runtime/VM contracts: `src/runtime_store.rs`, `src/roles/workflow_vm.rs`.
- Trait composition and prelude: `src/lib.rs`.
- SQL implementation: `../runinator-database/AGENTS.md`.

## Verification

```bash
cargo check -p runinator-store
cargo test -p runinator-store
```

Then test every implementation and primary caller affected by the changed trait; runtime-host
operations require `runinator-runtime` fake coverage.
