# Workspace storage

`runinator-workspace-storage` owns backend-neutral immutable storage algorithms. It must not
depend on `runinator-workspace`, SQL, HTTP, brokers, or service crates. `runinator-workspace`
interprets named results and materializes provider directories; the engine owns publication,
authorization, shared blob access, and retention policy.

- Object identities describe canonical uncompressed logical objects, never physical pack layout.
- Repacking must preserve logical identities and reconstruct physical delta dependencies.
- Namespace and path projection roots change together or neither changes.
- Transactions stage immutable objects; only the engine may publish a cluster workspace version.
- The filesystem repository is a reference backend, never cluster authority.
- Keep traversal, range reads, compression groups, and transfer buffers bounded.
- Retention roots are explicit; ancestry does not imply retention of ancestor contents.
- Keep portable path/link validation at materialization boundaries as well as import validation.

Run `cargo test -p runinator-workspace-storage`, including format, corruption, namespace,
compression, OCI, and reference-backend coverage. Run workspace/engine/database integration
tests when an object, receipt, or retention contract changes.
