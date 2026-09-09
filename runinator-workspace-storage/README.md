# Runinator workspace storage

Backend-neutral immutable revision algorithms adapted from `paged-workspace-v9` (MIT, 2026
paged-workspace contributors). The original license is retained in `LICENSE` and the supplied
format notes are preserved as `docs/REFERENCE_FORMAT.md`. Runinator format changes are described
in `docs/FORMAT.md`; the reference notes are not the production wire contract.

`ReadStore` / `WriteStore` expose logical objects. `transaction::Edit` stages namespace and
projection changes and creates an unpublished revision. `Staging` spools objects to disposable
disk without remote existence probes on writes. `packs::seal` emits bounded packs containing only
the selected reachable closure not already in a base store. `View` provides bounded directory
pages, sparse materialization and range reads;
`diff::page` provides resumable Merkle diffs. `gc::mark_roots` takes explicit retained roots.

Workers deduplicate outgoing packs against their verified local cache of remote objects. Objects
absent from that cache are included, avoiding one API request per new object; the engine still
validates uploaded packs and the complete revision before publication.

`record::read_indexed` uses a bounded decoded-record cache for local immutable packs. Shared
compression containers are verified once while resident; each requested member still has its
logical identity checked. This avoids rereading and hashing the full container for every chunk.

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
