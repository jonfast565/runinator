use super::*;

// input types ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    /// An awaitable durable-operation result. `None` represents the surface `task` generic
    /// elision; semantic analysis fills it from context when the type is used on a binding.
    Task(Option<Box<TypeExpr>>),
    Enum(Vec<runinator_models::value::Value>),
    Range {
        base: Box<TypeExpr>,
        min: Option<runinator_models::value::Value>,
        max: Option<runinator_models::value::Value>,
    },
    Array(Box<TypeExpr>),
    Map(Box<TypeExpr>),
    Struct {
        fields: Vec<TypeField>,
        additional: Option<Box<TypeExpr>>,
    },
    Union(Vec<TypeExpr>),
    /// a first-class function type `function<(params) -> ret>`, the surface form of the type a
    /// lambda infers. lowers to `RuninatorType::Function`.
    Function {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
}

// secrets (.rexraps) -----------------------------------------------------------

/// a single `.rexraps` declaration: `secret|config <scope>.<name…> = <literal>`. the value must be a
/// pure literal; lowering rejects references and interpolation.
#[derive(Debug, Clone, PartialEq)]
pub struct SecretDecl {
    pub is_config: bool,
    pub path: Vec<PathSeg>,
    pub value: Expr,
    pub span: Span,
}

// pipelines (.rexrapp) ---------------------------------------------------------

/// a directed link in a `.rexrapp` pipeline: `"A" -> "B" on <selector>`. `on` holds the raw selector
/// keyword (`success`/`complete`/`failure`) or `None` when omitted; lowering resolves it.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineLinkDecl {
    pub from: String,
    pub to: String,
    pub on: Option<String>,
    pub parameters: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineJoinDecl {
    pub target: String,
    pub mode: String,
    pub parameters: Option<Expr>,
    pub span: Span,
}

/// a pipeline-level trigger parsed from a `.rexrapp` header. `cron` carries the schedule for a cron
/// trigger; a chained trigger sets `event` (raw `on_success`/`on_failure`/`on_complete`), `source_kind`
/// (`workflow`/`pipeline`), and `source` (the source name). `disabled` toggles the enabled flag.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineTriggerDecl {
    pub cron: Option<String>,
    pub event: Option<String>,
    pub source_kind: Option<String>,
    pub source: Option<String>,
    pub disabled: bool,
    pub span: Span,
}

/// a `workflow "Name"` member declaration, optionally followed by `on_failure <mode>`. `on_failure`
/// holds the raw keyword (`stop`/`continue`/`silently_continue`/`inquire`) or `None` when the member
/// takes the pipeline's default failure mode; lowering maps it to [`PipelineMemberFailureMode`]
/// (`runinator_models::pipelines`).
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineMemberDecl {
    pub name: String,
    pub on_failure: Option<String>,
    pub span: Span,
}

/// a `pipeline "Name" { ... }` block parsed from a `.rexrapp` file. `on_failure` holds the raw policy
/// keyword (`halt`/`continue`) or `None`; lowering maps the string fields to the model enums.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineDecl {
    pub name: String,
    pub description: Option<String>,
    pub on_failure: Option<String>,
    pub max_depth: Option<u32>,
    pub members: Vec<PipelineMemberDecl>,
    pub links: Vec<PipelineLinkDecl>,
    pub joins: Vec<PipelineJoinDecl>,
    pub concurrency: Option<super::ConcurrencyDecl>,
    pub triggers: Vec<PipelineTriggerDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeField {
    pub name: String,
    pub optional: bool,
    pub ty: TypeExpr,
    /// an optional default expression, only present on top-level workflow parameter fields. when
    /// set the field is effectively optional and the expression fills it at run start if omitted.
    pub default: Option<Expr>,
    /// the source span of this field, used to attach comments for lossless formatting. defaults to an
    /// empty span for fields synthesized outside the parser.
    pub span: Span,
    /// leading/trailing/dangling comments on this `params`/`type` struct field.
    pub comments: CommentSet,
}
