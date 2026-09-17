use super::*;

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Action(ActionStmt),
    /// a call to a `task fn`, inlined at this site during lowering.
    TaskCall(TaskCallStmt),
    Compute(ComputeStmt),
    Subflow(SubflowStmt),
    Wait(WaitStmt),
    Output(OutputStmt),
    Yield(Expr),
    Input(InputStmt),
    Approval(ApprovalStmt),
    Gate(GateStmt),
    Signal(SignalStmt),
    Assert(AssertStmt),
    Transform(TransformStmt),
    Audit(AuditStmt),
    Checkpoint(CheckpointStmt),
    Mutex(MutexStmt),
    Throttle(ThrottleStmt),
    Cooldown(CooldownStmt),
    Await(AwaitStmt),
    Debounce(DebounceStmt),
    Collect(CollectStmt),
    Barrier(BarrierStmt),
    CircuitBreaker(CircuitBreakerStmt),
    EventSource(EventSourceStmt),
    Config(ConfigStmt),
    /// `return <expr>?` — supplies the run's result and continues to the generated `end` node.
    Return(Option<Expr>),
    /// `detach <handle>` — stop tracking an `async` task handle; it is never joined.
    Detach(String),
    Fail(Option<Expr>),
    If(IfStmt),
    For(ForStmt),
    While(WhileStmt),
    Match(MatchStmt),
    Parallel(ParallelStmt),
    Try(TryStmt),
    /// `resume`, `resume next`, `resume restart`, `resume fail` — ends an interrupt handler region.
    Resume(ResumeStmt),
    Race(RaceStmt),
    Map(MapStmt),
}

/// a transition destination. `done` and `fail` are reserved labels that resolve to the
/// synthetic terminal nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Label(String),
    /// the generated successful terminal, spelled `end` in source.
    End,
    /// the generated failing terminal, spelled `fail` in source.
    Fail,
}

/// a single line in a compute block.
#[derive(Debug, Clone, PartialEq)]
pub enum ComputeLine {
    Let {
        name: String,
        ty: Option<TypeExpr>,
        value: Expr,
    },
    Return(Expr),
    Goto(Target),
    If {
        cond: Cond,
        then_branch: Vec<ComputeLine>,
        else_branch: Vec<ComputeLine>,
    },
    Expr(Expr),
}

/// the wait duration: a literal count of seconds or an expression yielding seconds.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitAmount {
    Seconds(i64),
    Expr(Expr),
}

/// A durable join target. A workflow target selects runs by workflow/correlation; a task target
/// joins the exact detached subflow run carried by a prior `task[T]` binding.
#[derive(Debug, Clone, PartialEq)]
pub enum AwaitTarget {
    Workflow(String),
    Task(String),
}

/// which router a `match`-family statement lowers to: `switch` cases, a `toggle` on/off, or a
/// `percentage` weighted split. carried on `MatchStmt` so all three reuse the same arm plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchMode {
    Cases,
    Toggle,
    Percentage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPolicy {
    All,
    Any,
    FirstSuccess,
}

mod stmt;
pub use stmt::Stmt;

mod annotations;
pub use annotations::Annotations;

mod transition_clause;
pub use transition_clause::TransitionClause;

mod predicate_edge;
pub use predicate_edge::PredicateEdge;

mod task_call_stmt;
pub use task_call_stmt::TaskCallStmt;

mod modifiers;
pub use modifiers::Modifiers;

mod retry_config;
pub use retry_config::RetryConfig;

mod reentry;
pub use reentry::Reentry;

mod action_stmt;
pub use action_stmt::ActionStmt;

mod compute_stmt;
pub use compute_stmt::ComputeStmt;

mod foreign_do;
pub use foreign_do::ForeignDo;

mod subflow_stmt;
pub use subflow_stmt::SubflowStmt;

mod wait_stmt;
pub use wait_stmt::WaitStmt;

mod output_stmt;
pub use output_stmt::OutputStmt;

mod input_stmt;
pub use input_stmt::InputStmt;

mod approval_stmt;
pub use approval_stmt::ApprovalStmt;

mod gate_stmt;
pub use gate_stmt::GateStmt;

mod signal_stmt;
pub use signal_stmt::SignalStmt;

mod config_stmt;
pub use config_stmt::ConfigStmt;

mod assert_stmt;
pub use assert_stmt::AssertStmt;

mod transform_stmt;
pub use transform_stmt::TransformStmt;

mod audit_stmt;
pub use audit_stmt::AuditStmt;

mod checkpoint_stmt;
pub use checkpoint_stmt::CheckpointStmt;

mod mutex_stmt;
pub use mutex_stmt::MutexStmt;

mod throttle_stmt;
pub use throttle_stmt::ThrottleStmt;

mod cooldown_stmt;
pub use cooldown_stmt::CooldownStmt;

mod await_stmt;
pub use await_stmt::AwaitStmt;

mod debounce_stmt;
pub use debounce_stmt::DebounceStmt;

mod collect_stmt;
pub use collect_stmt::CollectStmt;

mod barrier_stmt;
pub use barrier_stmt::BarrierStmt;

mod circuit_breaker_stmt;
pub use circuit_breaker_stmt::CircuitBreakerStmt;

mod event_source_stmt;
pub use event_source_stmt::EventSourceStmt;

mod if_stmt;
pub use if_stmt::IfStmt;

mod for_stmt;
pub use for_stmt::ForStmt;

mod while_stmt;
pub use while_stmt::WhileStmt;

mod match_stmt;
pub use match_stmt::MatchStmt;

mod match_arm;
pub use match_arm::MatchArm;

mod parallel_stmt;
pub use parallel_stmt::ParallelStmt;

mod parallel_branch;
pub use parallel_branch::ParallelBranch;

mod try_stmt;
pub use try_stmt::TryStmt;

mod resume_stmt;
pub use resume_stmt::ResumeStmt;

mod race_stmt;
pub use race_stmt::RaceStmt;

mod map_stmt;
pub use map_stmt::MapStmt;
