# Runinator workspace storage

Backend-neutral immutable revision algorithms adapted from `paged-workspace-v9` (MIT, 2026
paged-workspace contributors). The original license is retained in `LICENSE` and the supplied
format notes are preserved as `docs/REFERENCE_FORMAT.md`. Runinator format changes are described
in `docs/FORMAT.md`; the reference notes are not the production wire contract.

`ReadStore` / `WriteStore` expose logical objects. `transaction::Edit` stages namespace and
projection changes and creates an unpublished revision. `Staging` spools objects to disposable
disk. `packs::seal` emits bounded packs containing only the selected reachable closure not already
in a base store. `View` provides bounded directory pages, sparse materialization and range reads;
`diff::page` provides resumable Merkle diffs. `gc::mark_roots` takes explicit retained roots.

`Repository` is a Unix filesystem reference implementation used by examples and tests. Runinator's
cluster uses engine-owned blob access, SQL object locations, validated receipts and fenced
publication. This crate must not depend on service, HTTP, SQL or broker implementations.

```sh
cargo test -p runinator-workspace-storage
cargo run -p runinator-workspace-storage --example workspace
cargo build -p runinator-workspace-storage --release --example qualification
/usr/bin/time -l target/release/examples/qualification 100 100000
```

The qualification example generates incompressible input without retaining it in memory, checks
bounded range access and pagination, and seals packs. It requires over 100 GiB of scratch disk.
On platforms without `/usr/bin/time -l`, use the platform's process memory measurement utility.
