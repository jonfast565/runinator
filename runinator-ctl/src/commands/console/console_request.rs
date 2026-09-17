#[allow(unused_imports)]
use super::*;

pub(crate) struct ConsoleRequest<'a> {
    pub requested_session: Option<&'a str>,
    pub new_session: Option<&'a str>,
    pub execute: Option<&'a str>,
    pub file: Option<&'a Path>,
    pub no_follow: bool,
    pub json_output: bool,
    pub api_base_url: &'a str,
    pub plain: bool,
}
