# AGENTS.md

Guidance for `runinator-database`. Read `runinator-store/AGENTS.md` first when changing a persistence
contract.

## Ownership

`runinator-database` owns the concrete SQLite, Postgres, and MariaDB implementations of
`runinator-store`: SQL, migrations, backend plumbing, and row conversion. Persistence traits and
plain exchange types belong to `runinator-store`; HTTP and orchestration behavior belong elsewhere.

## Boundaries and Invariants

- Add an operation to the owning store role first, then implement the body once on `SqlStore<B>` in
  the matching `operations/<role>.rs`. `SqliteDb`, `PostgresDb`, and `MariaDb` remain aliases.
- Keep shared SQLx row mapping in `src/mappers.rs` and shared dialect plumbing in
  `src/operations/mod.rs`.
- Each role implementation deliberately repeats the SQLx `where` bounds. Do not hide them in a
  macro or bundle trait: Rust does not treat a trait's `where` clause as implied bounds, and macro
  errors would lose useful source locations.
- Add every migration to the SQLite, Postgres, and MariaDB migration directories.
  `migration_parity_tests.rs` enforces matching versions; document a genuine dialect-only migration
  in its `DIALECT_ONLY` list.
- Add lifecycle coverage to shared `src/dialect_parity.rs`, not one backend's private suite.
  SQLite runs it unconditionally; live Postgres/MariaDB runs prove parity.

MariaDB uses SQLx's MySQL-protocol types and `mysql://` URLs. This crate's three backend
implementations must nevertheless expose the same persistence behavior. Database action-provider
decoding, including MariaDB JSON/boolean and exact decimals, is owned by
`runinator-provider-db/AGENTS.md`, not this persistence crate.

## Where to Start

- Contract roles: `../runinator-store/src/roles/`.
- Backend wrapper and shared query plumbing: `src/backend.rs`, `src/operations/mod.rs`.
- Row/type mapping: `src/mappers.rs`.
- Migrations: `migrations/{sqlite,postgres,mariadb}/`.
- Shared behavior: `src/dialect_parity.rs` and mapper tests.

## Verification

```bash
cargo check -p runinator-database
cargo test -p runinator-database
```

For schema or operation changes, run all live dialects:

```bash
docker compose -f runinator-database/tests/docker-compose.yml up -d --wait
RUNINATOR_TEST_POSTGRES_URL=postgres://runi:runi@127.0.0.1:55433/runi \
RUNINATOR_TEST_MARIADB_URL=mysql://root:runi@127.0.0.1:53307/runi \
  cargo test -p runinator-database --features sqlite,postgres,mariadb
docker compose -f runinator-database/tests/docker-compose.yml down -v
```
