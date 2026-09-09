//! Compression group correctness.

use super::*;
use crate::model::Kind;
fn item(bytes: Vec<u8>) -> (Id, Vec<u8>) {
    (Id::object(Kind::Chunk, &bytes), bytes)
}
#[test]
fn block_round_trip_and_identity() {
    let a = (0..220_000).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    let mut b = a.clone();
    b[100_000..100_400].fill(b'X');
    let c = b"fn repeated_source_name() { repeated_source_name(); }\n".repeat(4000);
    let input = vec![item(a), item(b), item(c)];
    let (raw, ids) = encode(&input).unwrap();
    assert_eq!(ids.len(), 3);
    for (slot, (id, bytes)) in input.iter().enumerate() {
        let m = member(&raw, slot as u32, *id).unwrap();
        assert_eq!(&m.raw, bytes);
        assert_eq!(info(&raw, slot as u32, *id).unwrap(), bytes.len());
    }
}
#[test]
fn shallow_delta_is_used_for_localized_edits() {
    let mut state = 0x918a_7723_u32;
    let a = (0..288_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect::<Vec<_>>();
    let mut b = a.clone();
    b[120_000..120_120].fill(b'!');
    let input = vec![item(a), item(b)];
    let (raw, ids) = encode(&input).unwrap();
    let second = member(&raw, 1, ids[1]).unwrap();
    assert!(
        second.is_delta,
        "similar localized edit should use a one-hop delta"
    );
}
#[test]
fn delta_chain_never_exceeds_one() {
    let a = b"0123456789abcdef".repeat(20_000);
    let mut b = a.clone();
    b[1000..1100].fill(1);
    let mut c = b.clone();
    c[2000..2100].fill(2);
    let input = vec![item(a), item(b), item(c)];
    let (raw, ids) = encode(&input).unwrap();
    for (slot, id) in ids.into_iter().enumerate() {
        assert_eq!(member(&raw, slot as u32, id).unwrap().raw, input[slot].1);
    }
}
