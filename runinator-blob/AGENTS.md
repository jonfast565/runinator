# AGENTS.md

Guidance for `runinator-blob` and sibling `runinator-blob-core`.

## Ownership

`runinator-blob-core` owns the backend-neutral `BlobStore` contract, keys/ranges/metadata/errors,
AWS signature-v4 canonicalization, and local-filesystem backend. It has no transport dependency.
Consumers of `Arc<dyn BlobStore>` depend on core.

`runinator-blob` owns the S3-compatible HTTP server/client, backend factory, and blob-service binary.
It re-exports core at historical paths. Signing and verification intentionally share the core
canonicalization; do not implement separate client/server algorithms.

## S3 and Artifact Invariants

- The S3 surface is intentionally partial: path-style addressing, SigV4 headers/presigned queries,
  object put/get-range/head/delete, bucket create/head/delete/list, ListObjectsV2, and multipart.
  Do not add virtual-host addressing, versioning, ACLs, or chunk-signed payloads without a caller.
- ETags are quoted SHA-256 values, not MD5 hashes.
- Artifact bytes live behind `BlobStore`, never on an individual runtime replica. The engine's
  `artifact_storage` module owns `blob://<bucket>/<key>` reads/writes and database references.
- Worker upload and result recording are separate by design: `/artifacts/content` stores bytes and
  records no row; the result-event path records metadata. Do not add a second endpoint that does
  both.

## Where to Start

- Contract, signing, local backend: `../runinator-blob-core/src/`.
- S3 client/server and factory: `src/`.
- Artifact orchestration: `../runinator-engine/src/artifact_storage.rs` and its scoped guide.

## Verification

```bash
cargo check -p runinator-blob-core -p runinator-blob
cargo test -p runinator-blob-core
cargo test -p runinator-blob
```

Protocol changes need client/server canonicalization and compatibility coverage; artifact changes
also require engine/worker tests.
