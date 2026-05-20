// public surface used by other crates (e.g., runinator-ws supervisor status endpoint).
// only the snapshot module + its types are exported; the rest stays private to the binary.

pub mod snapshot;
pub mod types;
