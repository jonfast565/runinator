#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct IngressAdmissionQuery {
    pub scope: String,
    pub correlation_key: String,
}
