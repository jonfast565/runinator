#[allow(unused_imports)]
use super::*;

pub struct PackReadinessRequest<'a> {
    pub org_id: Option<Uuid>,
    pub pipeline_namespace: &'a str,
    pub pipeline_keys: &'a [&'a str],
    pub execution_profile: &'a str,
    pub visible_pipeline_ids: Option<&'a std::collections::HashSet<Uuid>>,
}
