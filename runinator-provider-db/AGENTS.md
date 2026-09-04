# AGENTS.md

Guidance for `runinator-provider-db`, the outbound database action provider.

## Ownership

This provider executes database inspect/query/script/provision actions and converts connector rows
to portable Runinator values. It is not the control-plane persistence implementation; schema,
migrations, and workflow storage belong to `runinator-database`.

## SQL Decode Invariants

- MariaDB uses SQLx's MySQL-protocol driver and `mysql://` URLs. It reports JSON as longtext/BLOB
  and has no distinct boolean type: `boolean` and `tinyint(1)` share protocol metadata.
  `src/connector/sql/decode.rs` reconciles metadata with decoded values so reported `kind` never
  contradicts the value. Test against MariaDB 11.
- Decode DECIMAL/NUMERIC through `BigDecimal`, not `f64`. `decimal_to_json` emits a number only when
  it round-trips through `f64` unchanged; otherwise emit the exact digits as a string.
- Postgres numeric wire values may discard trailing-zero groups, so tests must not require scale or
  trailing-zero preservation.
- Keep connector-specific behavior inside the provider and portable table export in
  `runinator-data-export`; do not leak SQLx types into shared workflow models.

## Where to Start

- SQL connection/decoding: `src/connector/sql/`.
- Provider actions: `src/actions/`.
- Rowset/export shaping: `src/rowset.rs`, `src/export.rs`.
- Live connector suite: `tests/live.rs`, `tests/docker-compose.yml`.

## Verification

```bash
cargo check -p runinator-provider-db
cargo test -p runinator-provider-db
```

Run the live Postgres and MariaDB connector suite for SQL type/metadata changes. A skipped live suite
does not validate dialect behavior.
