#[allow(unused_imports)]
use super::*;

pub(super) struct DockerRun<'a> {
    pub(super) image: &'a str,
    pub(super) language: &'a str,
    pub(super) command: &'a [String],
    pub(super) work_dir: &'a Path,
    pub(super) context: &'a Value,
    pub(super) timeout_secs: i64,
    pub(super) environment: &'a BTreeMap<String, String>,
    pub(super) limits: &'a RuntimeLimits,
}
