# Runinator workspace object format v1

This format intentionally has no compatibility decoder for legacy Runinator gzip workspaces or
unmodified paged-workspace-v9 repositories. `REFERENCE_FORMAT.md` preserves the upstream design
notes; Rust encoders/decoders and format/corruption tests are the source of truth for byte fields.

Logical IDs use BLAKE3 with `runinator/workspace/object/v1\0` domain separation and an object-kind
tag. Pack transport digests and OCI descriptors use SHA-256. Logical IDs identify uncompressed
canonical object content and are independent of compression, grouping, pack order or locations.

Revision objects contain namespace workspace and path projection roots, an optional parent ID,
message, and an optional attachment radix root. The attachment radix maps UTF-8 named-result keys
to paged file objects. Namespace inode numbers preserve hard-link identity; path projection nodes
contain inode references and an optional radix root of child nodes. Directory updates do not
serialize every sibling. Namespace and projection changes are atomic within an edit.

Record/pack magic is `RNWREC01` / `RNWPACK1`. Decoding verifies lengths, object kinds, payload
checksums and typed logical IDs. Objects are at most 16 MiB; compression groups are at most 4 MiB
of decoded members, dictionaries at most 32 KiB, and delta bases are full members in the same
physical record. Tiny-file blocks, 512 KiB metadata blocks, and chunk groups retain independent
member IDs. Metadata blocks colocate small namespace, projection, revision, and file-manifest
objects without changing their logical IDs. Decoding a member may need its bounded physical group
and delta base, never an entire workspace archive.

Production packs target 64 MiB with an 80 MiB transport ceiling. Server-side indexing validates
uploaded records and derives every object location and raw kind/length; clients cannot submit
trusted physical offsets. Pack keys include an upload/collection scope UUID, avoiding reuse of a
physical key while old-generation cleanup is in flight.

GC follows explicit namespace/projection/attachment roots. Parent metadata is not a retention edge
unless the reference repository explicitly requests ancestry. Repacking reconstructs logical
objects before producing new groups and therefore preserves required physical delta dependencies.
