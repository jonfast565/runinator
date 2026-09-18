use reqwest::{Method, Url};
use serde_json::Value;

pub(crate) struct RequestSpec {
    pub(crate) method: Method,
    pub(crate) url: Url,
    pub(crate) query: Vec<(String, String)>,
    pub(crate) body: Option<Value>,
}
