# Native format 6 and invariants

This documents the v0.9 source implementation, **not verified conformance**. The
source has not been compiled or executed here. Metadata/decoded object limits
remain 16 MiB unless a smaller per-kind limit applies.

## Identity and canonical primitives

Internal object identity is a 32-byte digest:

```text
BLAKE3(
  ASCII("paged-workspace/object/v3") || 00 ||
  kind:u8 || raw_length:u64-little-endian || raw_payload
)
```

Compression and physical tiny-block grouping do not participate in a member's
logical identity. Physical packs, indexes, catalogs, and OCI blobs use SHA-256 of
exact serialized file bytes. Text IDs are 64 lowercase hex characters; OCI
adds the explicit `sha256:` prefix in descriptors.

Canonical metadata begins with byte **03**. Integers are little-endian except
numeric radix keys, which are big-endian to preserve ordered lookup. IDs are 32
raw bytes; byte vectors/UTF-8 strings use `length:u32 || bytes`; optional IDs use
`00` or `01 || id`. Decoders reject unsupported versions, malformed/trailing
fields, invalid UTF-8, unordered maps, and unsupported layout parameters.

| Kind | Payload |
|---:|---|
| 1 | Raw FastCDC data chunk |
| 2 | Page manifest with data/zero extents |
| 3 | Patricia radix node |
| 4 | FileObject: paged, inline, or tiny |
| 5 | Inode |
| 6 | Directory Link: inode number |
| 7 | Workspace |
| 8 | Revision |
| 9 | PathProjection |
| 10 | PathNode |
| 11 | Reserved legacy pathname-list kind; rejected by format-5 GC |
| 12 | Structural PathRef |
| 13 | Structural RefList |
| 14 | Physical TinyBlock container; forbidden as a logical graph edge |

TinyBlock is a physical container with its own header below, not an ordinary
canonical metadata payload. Its outer record still has a typed BLAKE3 identity.

## FileObject

After metadata version byte:

```text
logical_size:u64
page_size:u32, cdc_min:u32, cdc_target:u32, cdc_max:u32
normalization:u8 = 1
seed:u64 = 0
storage_tag:u8
  0: page_map_root:optional-id
  1: inline_bytes:byte-vector     # 0..128 bytes
  2: tiny_bytes:byte-vector       # 129..65536 bytes
```

For tags 1/2, payload byte length equals logical size and there is no page map.
In the Rust struct, `small: Some(bytes)` and `pages: Some(root)` are mutually
exclusive. A nonempty all-zero small file uses tag 0 with no page root instead
of storing zero bytes. Tag 0 is also permitted for explicitly paged small files.

The page map maps eight-byte big-endian page numbers to Page IDs. Missing entries
are sparse zero pages. Nonzero data past EOF or a mapping wholly past EOF is
invalid. No physical pack identity or disk offset is part of a FileObject.

Page sizes are powers of two in [4 KiB, 8 MiB]. CDC remains the pinned FastCDC
5.0.0 v2020 implementation, normalization Level1, seed 0. Default layout is 4 MiB
pages with min/target/max of 64 KiB / 256 KiB / 1 MiB. Sized ingestion chooses
1/4/8 MiB before ingestion using the README policy; layout remains fixed on writes.

## Page manifest

After version byte:

```text
page_size:u32
used_prefix_length:u32
extent_count:u32
repeat extent_count:
  tag:u8
    0: zero_length:u32
    1: chunk_id:32 || decoded_chunk_length:u32
```

`used` is nonzero and at most `page_size`. Extent lengths are nonzero and sum
exactly to `used`. The last extent must be data and the final reconstructed used
byte must be nonzero. Bytes after `used` are implicitly zero-filled.

Zero extents are at least **4096 bytes**, have no chunk ID, and cannot be adjacent.
Writers detect maximal such runs and run CDC independently on the data spans
between them. There are at most 131072 extents. An entirely zero page has no
manifest and no page-map entry. Reads and graph validation do not fetch objects
for zero extents. These checks do not require fsck to recompute CDC boundaries.

## Merkle metadata

These schemas are unchanged in structure from v0.7 except for the global version
byte/identity domain.

`RadixNode`: `prefix:byte-vector || value:optional-id || child_count:u16 || sorted
repeat [edge:u8, child_id:32]`. Child edge consumes one byte. Empty trees are
`None`; valueless single-child nodes are collapsed. Numeric keys are eight-byte
big-endian. Values reference typed objects, not inline untyped values.

`Inode`: `link_count:u64 || mode:u32 || created_ns:i64 || modified_ns:i64 ||
xattr_count:u32 || sorted repeat [name:string, value:bytes] || data_tag:u8`.
Data tags: 1 FileObject ID, 2 directory-name radix root (optional ID), 3 symlink
UTF-8 target. Names/timestamps/modes are stored metadata, not OS access checks.
Directory hard links are forbidden; regular-file aliases share an inode number.

`Link`: `inode_number:u64`.

`Workspace`: `inode_table_root:optional-id || next_inode_number:u64`. Root inode
is 1; allocated inode numbers are below `next_inode`. Semantic validation checks
reachability, exact hard-link counts, inode targets, and directory structure.

`PathNode`: `inode_number:u64 || inode_object_id:32 || child_count:u32 || sorted
repeat [name:string, child_path_node_id:32]`. Descendant content identity therefore
propagates to ancestor projection hashes.

`PathRef`: `parent_directory_inode:u64 || basename:string`. Parent relationships
are resolved within the corresponding revision, not against a global mutable
path table. Moving an ancestor does not rewrite descendant refs.

`RefList`: `count:u32 || sorted unique path_ref_id:32[count]`, at least two refs.

`PathProjection`: `root_path_node_id:32 || hardlink_index_root:optional-id ||
directory_ref_index_root:optional-id`. The hard-link radix maps inode numbers to
RefLists. The directory-ref radix maps each non-root directory inode to its parent
PathRef. Transactions maintain these roots together with workspace roots.

`Revision`: `parent:optional-id || workspace_id:32 || projection_id:32 ||
message:string(max 65536)`. Parents are retention/history edges, not patch replay
requirements. Each revision commits to a complete logical snapshot.

## Pack records

A pack starts with `PWPACK03` (8 bytes), with no outer compression stream.
Each physical record has this 96-byte header:

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 8 | `PWREC003` |
| 8 | 1 | Object kind |
| 9 | 1 | Codec: 0 raw, 1 zstd |
| 10 | 6 | Reserved zero |
| 16 | 8 | Decoded length, LE |
| 24 | 8 | Encoded length, LE |
| 32 | 32 | Typed BLAKE3 identity |
| 64 | 32 | BLAKE3 checksum of encoded bytes |
| 96 | variable | Encoded payload |

Writers try zstd level 3 for raw chunks and tiny containers, retaining it only
when smaller. Other metadata is raw. Decoded objects are limited to 16 MiB;
encoded lengths to 16 MiB + 64 KiB. Container payloads have a tighter 256-KiB limit.
Readers verify record bounds, encoded checksum, decoded length, and typed ID.
Unpublished transaction records use the same header without the pack magic.

## Physical TinyBlock

A kind-14 record contains at most **262144 decoded bytes**, including its table:

```text
magic:8 = PWTINY01
member_count:u32
repeat member_count in increasing File-ID order:
  file_object_id:32
  canonical_file_payload_length:u32
repeat member_count:
  canonical_file_payload:bytes_without_a_length_prefix
```

The count is 1..4096 and the table/ranges must consume the block exactly. Members
must be FileObjects with tag 2 (129 bytes through 64 KiB of logical file data).
Duplicates, invalid lengths, wrong File IDs, non-tiny members, and nesting are
rejected. Payloads include their normal canonical metadata version byte.

Each File ID is indexed independently. The block's ID is NOT entered as a logical
CAS object. Repacking can change the block and pack identity without changing any
member or revision ID. Logical graph traversal must reject a kind-14 target.
Native pack iteration expands the block into ordinary validated File objects.

Reads validate the requested member against the expected index ID as well as the
container record. A shared 4-MiB decoded block cache prevents repeatedly
re-decompressing the same <=256-KiB block. Ordinary metadata caching remains
keyed by the individual member's stable File ID.

## Compact global physical index

```text
magic:8 = PWINDEX3
object_count:u64
pack_count:u32
entry_width:u32 = 52
pack_sha256:32[pack_count]       # strictly increasing, at most 65536
entries[object_count]           # strictly increasing internal object IDs
```

Each entry is exactly **52 bytes**:

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 32 | Internal object ID |
| 32 | 4 | Pack dictionary slot, LE |
| 36 | 8 | Physical record offset, LE |
| 44 | 4 | Total physical record length including header, LE |
| 48 | 4 | Tiny member slot, or `0xffffffff` for a standalone object |

Exact file size: `24 + 32*pack_count + 52*object_count`. Pack slots, table ordering,
stride, arithmetic and file size are checked. Record lengths fit u32 under record
limits. For a tiny member, the location is its container record plus its member
slot. The runtime restores the pack's full SHA-256 using the dictionary.

Construction spools full 84-byte temporary Location records, retaining only a
bounded pack dictionary in RAM. External sorting still uses 4096-entry runs and
pairwise disk merges. Global-index merge translates dictionary slots and retains
old locations for duplicate IDs. This remains a sorted-file index rewritten per
commit, not a B+tree or an LSM tree.

## Catalog and atomic publication

Canonical version-3 catalog fields:

```text
generation:u64
index_sha256:optional-id
pack_count:u32 || sorted pack_sha256:32[pack_count]
ref_count:u32 || sorted repeat [ref_name:string, revision_id:32]
```

Pack/ref counts are at most 65536 and total metadata at most 16 MiB. Every index
pack must occur in the catalog. `CURRENT` is exactly the catalog SHA-256 as 64
lowercase hex characters and a newline. New packs, index, and catalog are durably
installed before replacing `CURRENT`. Failed pre-publication work may be garbage.
No physical grouping changes the optimistic ref-update protocol.

GC marks logical roots and members, repacks live objects into new tiny containers,
publishes the new index/catalog, then removes unreferenced old physical files.
Retained revision parents intentionally keep historical objects live.

## OCI envelope

Local OCI image-layout version remains 1.0.0. Native artifact media types are:

```text
application/vnd.paged-workspace.repository.v6
application/vnd.paged-workspace.config.v6+json
application/vnd.paged-workspace.pack.v6
```

The native config is `{"formatVersion":6,"revision":"<internal-id>"}`. Descriptors
use SHA-256 over exact blob bytes. Pack blobs can contain standalone records and
tiny containers; import expands/verifies members, validates the graph, and only
then publishes the original native revision. Standard OCI filesystem images still
use tar layers and are independent of this native format bump. Registry HTTP,
signing, and external-runtime conformance are not established by this package.


## ChunkBlock physical compression groups

`Kind::ChunkBlock = 15` is physical-only and is never a logical Merkle edge. The compact object index may route a logical `Kind::Chunk` ID to a ChunkBlock record plus member slot, exactly as tiny FileObjects can be routed to TinyBlock members.

A ChunkBlock starts with `PWCHNK01`, followed by `dictionary_len:u32`, `member_count:u32`, shared dictionary bytes (maximum 32 KiB), then member records. Each member contains the logical Chunk ID, raw length, codec, optional earlier base slot, payload length and payload. Codecs are raw, zstd-with-shared-dictionary, and one-hop prefix/suffix delta compressed with the shared dictionary. A delta base must be an earlier non-delta member; chains longer than one are rejected.

Groups are bounded to 256 members and approximately 4 MiB of uncompressed input. Similarity selection is physical-only: a deterministic four-lane 64-bit sketch indexes earlier full members inside the group. This sketch never participates in logical object identity.

ChunkBlock records are stored without another outer zstd pass. Normal pack-record BLAKE3 checksums still protect the encoded block bytes, and member decode recomputes the canonical typed Chunk ID from reconstructed raw bytes.
