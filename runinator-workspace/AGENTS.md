# Workspace integration

`runinator-workspace` depends on `runinator-workspace-storage` for all storage algorithms.
It owns named result encoding/resolution, local filesystem reconciliation and materialization,
and ordinary workspace archive projection. It does not publish cluster versions, own SQL,
authorize requests, or implement transports.

Apply server-issued limits with checked accounting. Preserve hard-link groups, sparse logical
lengths, and safe relative symlinks. Do not infer unchanged content from timestamps alone.
Run `cargo test -p runinator-workspace` and the storage tests; changes to provider directory
behavior also require worker integration coverage.
