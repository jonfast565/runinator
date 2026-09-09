//! Shared physical record decoding and corruption checks.
use super::*;
use crate::{cache::ByteCache, index::Location};
use std::io::{Seek, SeekFrom};

#[test]
fn indexed_chunk_members_decode_the_shared_record_once() -> Result<()> {
    let mut pending = crate::chunkblock::Pending::default();
    let expected = [vec![1; 65536], vec![2; 32768]];
    for raw in &expected {
        pending.push(Id::object(Kind::Chunk, raw), raw)?;
    }
    let (raw, ids) = pending.take()?.unwrap();
    let mut pack = tempfile::NamedTempFile::new()?;
    let (_, length) = write(&mut pack, Kind::ChunkBlock, &raw)?;
    let cache = ByteCache::new(1024 * 1024);
    for (slot, id) in ids.iter().enumerate() {
        let location = Location {
            id: *id,
            pack: Id::default(),
            offset: 0,
            length,
            member: slot as u32,
        };
        assert_eq!(
            *read_indexed(pack.as_file(), location, &cache)?.bytes,
            expected[slot]
        );
    }
    assert_eq!(cache.stats()?.misses, 1);
    assert_eq!(cache.stats()?.hits, 1);
    pack.seek(SeekFrom::Start(HEADER_LEN))?;
    pack.write_all(&[0])?;
    let fresh = ByteCache::new(1024 * 1024);
    assert!(
        read_indexed(
            pack.as_file(),
            Location {
                id: ids[0],
                pack: Id::default(),
                offset: 0,
                length,
                member: 0
            },
            &fresh
        )
        .is_err()
    );
    Ok(())
}
