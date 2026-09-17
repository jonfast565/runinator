#[allow(unused_imports)]
use super::*;

pub struct Page {
    pub changes: Vec<Change>,
    pub cursor: Option<Cursor>,
}
