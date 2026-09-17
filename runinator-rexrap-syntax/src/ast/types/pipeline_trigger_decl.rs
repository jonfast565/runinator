#[allow(unused_imports)]
use super::*;

/// a pipeline-level trigger parsed from a `.rexrapp` header. `cron` carries the schedule for a cron
/// trigger; a chained trigger sets `event` (raw `on_success`/`on_failure`/`on_complete`), `source_kind`
/// (`workflow`/`pipeline`), and `source` (the source name). `disabled` toggles the enabled flag.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineTriggerDecl {
    pub cron: Option<String>,
    pub schedule: Option<Expr>,
    pub exclusions: Vec<Expr>,
    pub event: Option<String>,
    pub source_kind: Option<String>,
    pub source: Option<String>,
    pub disabled: bool,
    pub span: Span,
}
