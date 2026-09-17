#[allow(unused_imports)]
use super::*;

pub struct Change {
    pub path: String,
    pub left: Option<Id>,
    pub right: Option<Id>,
    pub result: bool,
}
