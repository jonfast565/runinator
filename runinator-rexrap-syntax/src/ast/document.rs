use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Whether the source explicitly identified itself with `language rexrap-1`.
    pub language_header: bool,
    /// top-level `fn` definitions, callable from the workflow body, compute blocks, and other
    /// function bodies. siblings of the workflow.
    pub functions: Vec<FunctionDef>,
    /// Pack-local, compile-time-only function modules. Imports select these by path; lowering
    /// embeds their resolved functions and digest into consuming workflows, never as an artifact.
    pub modules: Vec<SourceModule>,
    pub workflows: Vec<Workflow>,
    /// comments after the last top-level item, preserved for lossless formatting.
    pub trailing_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceModule {
    pub path: String,
    pub functions: Vec<FunctionDef>,
    pub span: Span,
}

impl Document {
    pub fn single_workflow(&self) -> Option<&Workflow> {
        self.workflows.first().filter(|_| self.workflows.len() == 1)
    }

    pub fn single_workflow_mut(&mut self) -> Option<&mut Workflow> {
        if self.workflows.len() == 1 {
            self.workflows.first_mut()
        } else {
            None
        }
    }
}

/// a top-level `fn name(params) -> ret = body` definition. the body is either a single expression
/// or a compute-style statement block; `recursive` carries the `@recursive(max_depth: N)` cap when
/// present.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    /// `task fn` — the body contains runtime work and is inlined into the graph. Callers still
    /// choose the scheduling (`f(...)` vs `async f(...)`), so this is a capability, not a color.
    pub is_task: bool,
    pub params: Vec<FnParam>,
    pub ret: Option<TypeExpr>,
    pub body: FnBody,
    pub recursive: Option<u32>,
    pub span: Span,
    /// leading/trailing comments, preserved for lossless formatting.
    pub comments: CommentSet,
}

/// a function body: a single expression (`= expr`) or a compute-style block of statements
/// (`= { let …; … ; return e }`). block bodies reuse the compute-line grammar and lower to the same
/// `$let`/`$return`/`$if` program form a `do` block produces.
#[derive(Debug, Clone, PartialEq)]
pub enum FnBody {
    Expr(Box<Expr>),
    Block(Vec<ComputeLine>),
    /// a `task fn` body: `do { … }`, a region of runtime statements inlined at each call site.
    Run(Block),
}

/// a function parameter: a typed name, optionally marked `?` or given a `= default` (both make it
/// omittable at the call site).
#[derive(Debug, Clone, PartialEq)]
pub struct FnParam {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workflow {
    pub name: String,
    /// Stable logical key, independent of display name and namespace. Parsing retains `None` so
    /// semantic analysis can report a precise missing-key diagnostic; lowering rejects it.
    pub key: Option<String>,
    pub version: Option<SemVer>,
    /// top-level workflow parameters, surfaced in source as `params { ... }`.
    pub input: Option<TypeExpr>,
    /// optional typed value produced in the subflow snapshot `state` field.
    pub output: Option<TypeExpr>,
    /// header `alias <name> = { ... }` declarations; reusable argument groups expanded into
    /// action calls by `...name` spreads during desugaring.
    pub aliases: Vec<Alias>,
    /// namespace this workflow's identity lives in, supplied by a declaration or enclosing block.
    /// Parsing retains `None` so semantic analysis can report it; lowering rejects it.
    pub namespace: Option<String>,
    /// header `import <path> (as <alias>)?` declarations opening namespaces into local scope.
    pub imports: Vec<Import>,
    /// an optional explicit `start -> <target>` entry edge. when `None` the first body
    /// statement is the entry; when set it names the entry node directly.
    pub start: Option<Target>,
    /// header `trigger cron "..."` declarations scheduling runs of this workflow.
    pub triggers: Vec<TriggerDecl>,
    /// header `notify on <event> -> <channel> "..."` failure-alerting policies for this workflow.
    pub notifications: Vec<NotifyDecl>,
    /// optional header `concurrency <n> on_conflict <policy>`: how many runs of this workflow may
    /// overlap, and what a firing does once the cap is reached.
    pub concurrency: Option<ConcurrencyDecl>,
    /// header `watch <cond> -> <target>` cancellation guards, evaluated on every reducer drive.
    pub watches: Vec<WatchDecl>,
    /// header `interrupt on <source> { ... }` handler regions.
    pub interrupts: Vec<InterruptDecl>,
    /// optional header `correlate key <expr>`: the value this workflow's runs are awaitable by. rides
    /// in `metadata.correlation` and is stamped onto each run's correlation key as it progresses.
    pub correlation: Option<Expr>,
    /// provider-neutral correlation-key ingress policy.
    pub ingress: Option<IngressDecl>,
    /// header `type <Name> ...` declarations: reusable named types.
    pub type_decls: Vec<TypeDecl>,
    /// the statements of the workflow's `do { … }` runtime block.
    pub body: Block,
    /// `join <name> { … }` named continuations, reachable only by an explicit `continue <name>`.
    pub joins: Vec<JoinDecl>,
    pub span: Span,
    /// comments before the `workflow` keyword, preserved for lossless formatting.
    pub leading_comments: Vec<Comment>,
    /// comments after the last body statement, before the closing brace.
    pub dangling_comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngressDecl {
    pub scope: String,
    pub routes: Vec<IngressRouteDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngressRouteDecl {
    pub event_type: String,
    pub lifecycle: String,
    pub action: String,
    pub intent: Option<String>,
    pub predicates: Vec<IngressPredicateDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngressPredicateDecl {
    pub pointer: String,
    pub operator: String,
    pub value: Option<Expr>,
    pub span: Span,
}

/// a `join <name> { … }` named continuation: a labelled region a `continue <name>` route enters.
/// unlike a fallthrough sibling it is never reached implicitly.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinDecl {
    pub name: String,
    pub body: Block,
    pub span: Span,
    pub comments: CommentSet,
}

/// a header `type <Name> { ... }` (struct shorthand) or `type <Name> = <type>` (alias) declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
    pub comments: CommentSet,
}

/// which terminal state of the source workflow fires a chained trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainEvent {
    Success,
    Failure,
    Complete,
}

impl ChainEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            ChainEvent::Success => "success",
            ChainEvent::Failure => "failure",
            ChainEvent::Complete => "complete",
        }
    }

    /// the `on_<event> workflow` keyword this event renders as.
    pub fn keyword(self) -> &'static str {
        match self {
            ChainEvent::Success => "on_success",
            ChainEvent::Failure => "on_failure",
            ChainEvent::Complete => "on_complete",
        }
    }
}

/// the kind-specific payload of a header trigger declaration.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // syntax nodes stay directly pattern-matchable throughout the compiler.
pub enum TriggerDeclKind {
    /// `trigger cron <schedule>`: `schedule` is a string expression (the cron expression), with an
    /// optional blackout window and catch-up policy.
    Cron {
        schedule: Expr,
        blackout_start: Option<Expr>,
        blackout_end: Option<Expr>,
        catchup: Option<CatchupDecl>,
    },
    /// `trigger on_<event> workflow <target>`: start `target` when this workflow run reaches the
    /// matching terminal state.
    Chained { event: ChainEvent, target: Expr },
}

/// what a cron trigger does with slots that came due while nothing was firing them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CatchupPolicy {
    /// collapse the backlog into a single run. the runtime default.
    #[default]
    FireOnce,
    /// replay each missed slot as its own run, up to `max`.
    FireAll,
    /// abandon slots later than `grace` and re-anchor to the next future one.
    Skip,
}

impl CatchupPolicy {
    /// the keyword this policy renders as, which is also its runtime name.
    pub fn keyword(self) -> &'static str {
        match self {
            CatchupPolicy::FireOnce => "fire_once",
            CatchupPolicy::FireAll => "fire_all",
            CatchupPolicy::Skip => "skip",
        }
    }
}

/// a `catchup <policy> [grace <duration>] [max <n>]` option on a header cron trigger. `grace`
/// applies to `skip` (how late a slot may be before it is abandoned) and `max` to `fire_all` (how
/// many missed slots one pass replays).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatchupDecl {
    pub policy: CatchupPolicy,
    pub grace_seconds: Option<i64>,
    pub max_slots: Option<i64>,
}

/// what a firing does when the workflow is already at its concurrency cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConcurrencyPolicy {
    /// start the run anyway. the runtime default, and what an absent `concurrency` header means.
    Allow,
    /// drop the slot and move on.
    #[default]
    Skip,
    /// hold the slot due until capacity frees up, without creating anything.
    Queue,
    /// cancel the in-flight runs and start this one.
    CancelPrevious,
}

impl ConcurrencyPolicy {
    /// the keyword this policy renders as, which is also its runtime name.
    pub fn keyword(self) -> &'static str {
        match self {
            ConcurrencyPolicy::Allow => "allow",
            ConcurrencyPolicy::Skip => "skip",
            ConcurrencyPolicy::Queue => "queue",
            ConcurrencyPolicy::CancelPrevious => "cancel_previous",
        }
    }
}

/// a header `concurrency <n> on_conflict <policy>` declaration. the policy defaults to `skip`:
/// writing a cap at all means the overlap is unwanted.
#[derive(Debug, Clone, PartialEq)]
pub struct ConcurrencyDecl {
    pub max_concurrent_runs: i64,
    pub on_conflict: ConcurrencyPolicy,
    pub span: Span,
    pub comments: CommentSet,
}

/// a header `trigger ...` declaration. `params` is the optional run parameter object shared by both
/// kinds; `kind` carries the cron schedule or the chaining target.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerDecl {
    pub kind: TriggerDeclKind,
    pub params: Option<Expr>,
    pub enabled: bool,
    pub span: Span,
    pub comments: CommentSet,
}

/// the runtime condition a header `notify` policy fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyEvent {
    Failure,
    RetryExhausted,
    Sla,
    Parked,
}

impl NotifyEvent {
    /// the keyword this event renders as.
    pub fn keyword(self) -> &'static str {
        match self {
            NotifyEvent::Failure => "failure",
            NotifyEvent::RetryExhausted => "retry_exhausted",
            NotifyEvent::Sla => "sla",
            NotifyEvent::Parked => "parked",
        }
    }

    /// the runtime `NotificationEvent` name this lowers to.
    pub fn runtime_name(self) -> &'static str {
        match self {
            NotifyEvent::Failure => "run_failed",
            NotifyEvent::RetryExhausted => "node_retry_exhausted",
            NotifyEvent::Sla => "run_sla_breached",
            NotifyEvent::Parked => "run_parked",
        }
    }

    /// duration events are evaluated by a periodic scan and are meaningless without a threshold.
    pub fn requires_threshold(self) -> bool {
        matches!(self, NotifyEvent::Sla | NotifyEvent::Parked)
    }
}

/// where a header `notify` policy delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyChannel {
    Slack,
    Email,
    App,
}

impl NotifyChannel {
    pub fn keyword(self) -> &'static str {
        match self {
            NotifyChannel::Slack => "slack",
            NotifyChannel::Email => "email",
            NotifyChannel::App => "app",
        }
    }

    /// the runtime `NotificationChannel` name this lowers to.
    pub fn runtime_name(self) -> &'static str {
        match self {
            NotifyChannel::Slack => "slack",
            NotifyChannel::Email => "email",
            NotifyChannel::App => "in_app",
        }
    }
}

/// a header `notify on <event> -> <channel> <target>` declaration. `after` carries the threshold
/// seconds for the duration events; `severity` defaults to `warning` when omitted.
#[derive(Debug, Clone, PartialEq)]
pub struct NotifyDecl {
    pub event: NotifyEvent,
    pub channel: NotifyChannel,
    pub target: Expr,
    pub after_seconds: Option<i64>,
    pub severity: Option<String>,
    /// optional `with { ... }` provider configuration overriding the generated delivery fields.
    pub configuration: Option<Expr>,
    pub enabled: bool,
    pub span: Span,
    pub comments: CommentSet,
}

/// a header `watch <cond> -> <target>` guard: when `cond` holds, the run jumps to `handler`.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchDecl {
    pub cond: Cond,
    pub handler: Target,
}

/// a header `interrupt on <source> [every <duration>] { ... }` handler. the block is a region: its
/// first statement is where the interrupt enters, and every path out of it ends at a `resume`.
#[derive(Debug, Clone, PartialEq)]
pub struct InterruptDecl {
    /// the author-facing source name (`wake`). kept as a string so a source this binary does not
    /// know is a lowering-time diagnostic rather than a parse failure.
    pub source: String,
    /// Required only for `interrupt on timer`: the repeating cadence measured from run start.
    pub interval_seconds: Option<i64>,
    pub enabled: bool,
    pub body: Block,
}

/// The artifact family opened by a typed import. An absent kind is reserved for `std` imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Workflow,
    Functions,
    Settings,
    Module,
}

impl ImportKind {
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Functions => "functions",
            Self::Settings => "settings",
            Self::Module => "module",
        }
    }
}

/// a header `import [kind] <path> [@revision(N)] (as <alias>)?` declaration. `path` is the
/// dotted namespace (`std.strings`, `acme.billing.reconcile`); `alias` binds a short local name.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub kind: Option<ImportKind>,
    pub path: String,
    /// `@revision(N)` is meaningful for workflow imports. It is resolved to a UUID + digest by
    /// pack import, never treated as a runtime name lookup.
    pub revision: Option<i64>,
    pub alias: Option<String>,
    pub span: Span,
    pub comments: CommentSet,
}

/// a header `alias <name> = { k: expr, ... }` binding: a named, reusable group of argument
/// values spread into action calls with `...name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    pub name: String,
    pub entries: Vec<(String, Expr)>,
    pub span: Span,
    pub comments: CommentSet,
}
