use super::*;

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
pub enum TriggerDeclKind {
    /// `trigger cron <schedule>`: `schedule` is a string expression (the cron expression), with an
    /// optional blackout window and catch-up policy.
    Cron {
        schedule: Expr,
        blackout_start: Option<Expr>,
        blackout_end: Option<Expr>,
        catchup: Option<CatchupDecl>,
    },
    /// `trigger schedule { ... }`: a portable `ScheduleSpec` object, with zero or more recurring
    /// schedule objects excluding occurrences.
    Schedule {
        schedule: Expr,
        exclusions: Vec<Expr>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyChannel {
    Slack,
    Email,
    App,
    Provider { provider: String, function: String },
}

impl NotifyChannel {
    pub fn keyword(&self) -> String {
        match self {
            NotifyChannel::Slack => "slack".into(),
            NotifyChannel::Email => "email".into(),
            NotifyChannel::App => "app".into(),
            NotifyChannel::Provider { provider, function } => format!("{provider}.{function}"),
        }
    }

    /// the runtime `NotificationChannel` name this lowers to.
    pub fn runtime_name(&self) -> &'static str {
        match self {
            NotifyChannel::Slack => "slack",
            NotifyChannel::Email => "email",
            NotifyChannel::App | NotifyChannel::Provider { .. } => "in_app",
        }
    }

    pub fn provider_binding(&self) -> Option<(&str, &str)> {
        match self {
            NotifyChannel::Provider { provider, function } => Some((provider, function)),
            _ => None,
        }
    }
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

mod document;
pub use document::Document;

mod source_module;
pub use source_module::SourceModule;

mod function_def;
pub use function_def::FunctionDef;

mod fn_param;
pub use fn_param::FnParam;

mod workflow;
pub use workflow::Workflow;

mod ingress_decl;
pub use ingress_decl::IngressDecl;

mod ingress_route_decl;
pub use ingress_route_decl::IngressRouteDecl;

mod ingress_predicate_decl;
pub use ingress_predicate_decl::IngressPredicateDecl;

mod join_decl;
pub use join_decl::JoinDecl;

mod type_decl;
pub use type_decl::TypeDecl;

mod catchup_decl;
pub use catchup_decl::CatchupDecl;

mod concurrency_decl;
pub use concurrency_decl::ConcurrencyDecl;

mod trigger_decl;
pub use trigger_decl::TriggerDecl;

mod notify_decl;
pub use notify_decl::NotifyDecl;

mod watch_decl;
pub use watch_decl::WatchDecl;

mod interrupt_decl;
pub use interrupt_decl::InterruptDecl;

mod import;
pub use import::Import;

mod alias;
pub use alias::Alias;
