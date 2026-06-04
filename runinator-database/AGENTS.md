# AGENTS.md

Guidance for agents working in `runinator-database`.

## Ownership

`runinator-database` owns persistence interfaces and concrete SQLite/Postgres implementations. Database-specific SQL, schema mapping, row conversion, and durable outbox/ready-node storage belong here, not in `runinator-ws`.

## Where To Start

- Public persistence contract: `src/interfaces.rs`.
- Shared row/type mapping: `src/mappers.rs`.
- Shared query text/helpers: `src/queries.rs`, `src/common.rs`.
- SQLite backend: `src/sqlite.rs`.
- Postgres backend: `src/postgres.rs`.
- Mapper and backend tests: `src/mappers_tests.rs`, `src/sqlite_tests.rs`.

## Boundaries

- Add persistence operations to `DatabaseImpl` first, then implement SQLite and Postgres together.
- Keep shared model structs in `runinator-models`; do not define database-local duplicates for wire/domain payloads.
- Keep SQLx row mapping centralized in `mappers.rs` when a mapping is shared or reused.
- Preserve backend-neutral behavior for ready nodes, action dispatch outbox rows, workflow runs, node runs, artifacts, provider catalog items, and automation records.
- Do not call broker, HTTP, worker, waker, or provider code from this crate.

## Verification

Use:

```bash
cargo check -p runinator-database
cargo test -p runinator-database
```

If a shared model field or serialized payload changed, also check `runinator-ws`, `runinator-api`, and command-center consumers.
