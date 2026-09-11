//! Physical metadata block ordering and bulk member decoding.

use crate::{Id, metadata_block::Pending, model::Kind, record, store::Object};
use std::sync::Arc;

#[test]
fn metadata_blocks_round_trip_ordered_logical_members() -> crate::Result<()> {
    let logical = [
        Object {
            kind: Kind::PathNode,
            bytes: Arc::new(b"path node".to_vec()),
        },
        Object {
            kind: Kind::Inode,
            bytes: Arc::new(b"inode".to_vec()),
        },
        Object {
            kind: Kind::Revision,
            bytes: Arc::new(b"revision".to_vec()),
        },
    ];
    let mut pending = Pending::default();
    for object in &logical {
        pending.push(
            Id::object(object.kind, &object.bytes),
            object.kind,
            &object.bytes,
        )?;
    }
    let (raw, ids) = pending.take()?.expect("metadata block");
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));

    let mut record_bytes = Vec::new();
    record::write(&mut record_bytes, Kind::MetadataBlock, &raw)?;
    let requests = ids
        .iter()
        .enumerate()
        .map(|(slot, id)| (*id, slot as u32))
        .collect::<Vec<_>>();
    let decoded = record::decode_ranges(&record_bytes, &requests)?;
    for (id, object) in ids.into_iter().zip(decoded) {
        assert_eq!(Id::object(object.kind, &object.bytes), id);
    }
    Ok(())
}
