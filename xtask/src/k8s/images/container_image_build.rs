#[allow(unused_imports)]
use super::*;

pub struct ContainerImageBuild<'a> {
    pub repository: Option<&'a str>,
    pub tag: &'a str,
    pub include_names: Option<&'a [&'a str]>,
    pub exclude_names: Option<&'a [&'a str]>,
    pub push_images: bool,
    pub database_backend: &'a str,
    pub broker_backend: &'a str,
}
