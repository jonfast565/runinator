#[allow(unused_imports)]
use super::*;

/// Controls Merkle-aware history projection into conventional OCI layers.
///
/// `max_layers` bounds the exported OCI stack. If the native revision chain is
/// longer, the oldest selected revision is emitted as a full checkpoint and
/// only newer revisions become delta layers. A value of 1 is equivalent to the
/// flattened `export_image()` representation.
#[derive(Debug, Clone)]
pub struct MerkleImageOptions {
    pub image: ImageOptions,
    pub max_layers: usize,
    pub include_empty_layers: bool,
}

impl Default for MerkleImageOptions {
    fn default() -> Self {
        Self {
            image: ImageOptions::default(),
            max_layers: 16,
            include_empty_layers: false,
        }
    }
}
