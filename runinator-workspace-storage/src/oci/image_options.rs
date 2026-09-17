#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct ImageOptions {
    pub os: String,
    pub architecture: String,
    pub reference_name: String,
    pub labels: BTreeMap<String, String>,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            os: "linux".into(),
            architecture: "amd64".into(),
            reference_name: "latest".into(),
            labels: BTreeMap::new(),
        }
    }
}
