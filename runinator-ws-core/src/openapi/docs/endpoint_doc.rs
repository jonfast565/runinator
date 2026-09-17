#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub struct EndpointDoc {
    pub method: &'static str,
    pub path: &'static str,
    pub tag: &'static str,
    pub summary: &'static str,
    pub description: &'static str,
    pub policy: EndpointPolicy,
    pub request: Option<RequestDoc>,
    pub query: &'static [ParamDoc],
    pub success_status: u16,
    pub success_description: &'static str,
    pub response_example: Example,
}
