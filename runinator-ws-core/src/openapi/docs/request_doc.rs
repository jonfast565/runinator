#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub struct RequestDoc {
    pub description: &'static str,
    pub example: Example,
    pub content_type: &'static str,
}
