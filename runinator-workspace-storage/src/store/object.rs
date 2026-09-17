#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug)]
pub struct Object {
    pub kind: Kind,
    pub bytes: Arc<Vec<u8>>,
}
