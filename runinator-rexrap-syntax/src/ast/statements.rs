use super::*;

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub span: Span,
    pub annotations: Annotations,
    /// `node <label> <- ...`; the binding doubles as the generated node id for leaf nodes.
    pub label: Option<String>,
    /// an optional `node <label>: <type> <- ...` annotation declaring the step's output type.
    pub label_type: Option<TypeExpr>,
    pub kind: StmtKind,
    /// `async <call>`: schedule this step as a task instead of joining it inline. Asyncness is a
    /// property of the call site, never of the callee, so no callable ever needs a second version.
    pub is_async: bool,
    pub transitions: TransitionClause,
    /// `compensate <call>` on an action node: the compensating action run in reverse on saga rollback.
    pub compensation: Option<Box<ActionStmt>>,
    /// leading/trailing/dangling comments, preserved for lossless formatting.
    pub comments: CommentSet,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotations {
    pub id: Option<String>,
    pub skip: bool,
    pub locked: bool,
    pub timeout_seconds: Option<i64>,
}

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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TransitionClause {
    pub next: Option<Target>,
    pub on_success: Option<Target>,
    pub on_failure: Option<Target>,
    pub on_timeout: Option<Target>,
    pub on_reject: Option<Target>,
    /// user-defined predicate edges, in declaration order; lowered to `transitions.branches`.
    pub branches: Vec<PredicateEdge>,
}

impl TransitionClause {
    pub fn is_empty(&self) -> bool {
        self.next.is_none()
            && self.on_success.is_none()
            && self.on_failure.is_none()
            && self.on_timeout.is_none()
            && self.on_reject.is_none()
            && self.branches.is_empty()
    }
}

/// a user-defined predicate edge: take `target` when `when` holds. `priority` orders evaluation
/// among predicate edges (lower first); `None` keeps declaration order.
#[derive(Debug, Clone, PartialEq)]
pub struct PredicateEdge {
    pub when: Cond,
    pub target: Target,
    pub priority: Option<i64>,
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

/// `name(args)` where `name` is a `task fn`. the arguments are bound by substitution when the
/// body is inlined, so a call site never pays for a child run.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCallStmt {
    pub name: String,
    pub args: Vec<(String, Expr)>,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Modifiers {
    pub timeout_seconds: Option<i64>,
    pub retry: Option<RetryConfig>,
    pub tags: Vec<String>,
    pub mcp: bool,
    pub reentry: Option<Reentry>,
    /// `.runner("<type>")`: require a worker carrying the `runner=<type>` label to execute this node.
    pub runner: Option<String>,
    /// `@workspace(<expr>)`: route this call to the stable worker instance held in a
    /// `WorkspaceAffinity` value.
    pub workspace_affinity: Option<Expr>,
    /// `.idempotent(key: <expr>)`: name this action's external effect. the reducer resolves the
    /// expression per dispatch and the worker reserves the result, replaying a recorded outcome
    /// instead of invoking the provider twice for the same key.
    pub idempotency_key: Option<Expr>,
}

/// `.retry(max, backoff: <s>, max: <s>, jitter: <bool>, on: any|failure|timeout)`. only `max` is
/// required; the rest fall back to the model defaults (base 1s, cap 300s, no jitter, retry any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryConfig {
    pub max_attempts: i64,
    pub backoff_base_seconds: Option<i64>,
    pub backoff_max_seconds: Option<i64>,
    pub jitter: bool,
    /// `any` | `failure` | `timeout`; `None` keeps the default (`any`).
    pub retry_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reentry {
    pub max_visits: i64,
    pub on_exhausted: Option<Target>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionStmt {
    pub provider: String,
    pub function: String,
    /// argument entries in source order. a `...alias` spread is carried as an entry whose value
    /// is `ExprKind::Spread`; desugaring expands it in place before sema and lowering.
    pub args: Vec<(String, Expr)>,
    pub modifiers: Modifiers,
}

/// an imperative `do { ... }` block. lowers to a `std.run`/`std.exec` action node.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeStmt {
    pub body: Vec<ComputeLine>,
    pub foreign: Option<ForeignDo>,
    pub modifiers: Modifiers,
}

/// a verbatim foreign-language compute block. lowers to `std.code` and runs on a worker.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignDo {
    pub language: String,
    pub source: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct SubflowStmt {
    pub workflow_name: String,
    /// A workflow-import revision selector, carried through codegen as a temporary reference until
    /// the importer replaces it with an exact UUID/digest pin.
    pub revision: Option<i64>,
    /// Set by namespace resolution when the target came from a typed workflow import. Its
    /// signature is supplied by the package resolver, so an offline authoring compile may retain
    /// an `Any` interface rather than rejecting a path it cannot inspect locally.
    pub imported: bool,
    /// `detached: true` => fire-and-forget; otherwise wait.
    pub detached: bool,
    pub reuse: bool,
    pub run_name: Option<Expr>,
    pub params: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaitStmt {
    pub amount: WaitAmount,
    pub until_status: Option<String>,
    pub initial_status: Option<String>,
}

/// the wait duration: a literal count of seconds or an expression yielding seconds.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitAmount {
    Seconds(i64),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutputStmt {
    pub event_type: Option<String>,
    pub data: Option<Expr>,
    /// artifact declarations from `name = expr` lines in the output block.
    pub items: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputStmt {
    pub prompt: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalStmt {
    pub approval_type: Option<String>,
    pub prompt: Expr,
    pub metadata: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GateStmt {
    pub kind: String,
    pub when: Option<Cond>,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
    pub timeout_policy: Option<String>,
    pub metadata: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalStmt {
    pub name: String,
    /// `key <expr>`: a correlation value resolved at park time so external webhooks can route here.
    pub correlation_key: Option<Expr>,
    pub metadata: Vec<(String, Expr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigStmt {
    pub name: Option<Expr>,
    pub metadata: Option<Expr>,
}

/// `assert { "name": cond, ... }`: named boolean invariants checked inline.
#[derive(Debug, Clone, PartialEq)]
pub struct AssertStmt {
    /// each entry is a (name, condition); the violation message defaults to the name.
    pub assertions: Vec<(String, Cond)>,
}

/// `transform { name = expr, ... }`: named context bindings reshaped from the runtime context.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformStmt {
    pub bindings: Vec<(String, Expr)>,
}

/// `audit action <expr> (actor <expr>)? (target <expr>)? (reason <expr>)?`: a compliance record.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditStmt {
    pub action: Expr,
    pub actor: Option<Expr>,
    pub target: Option<Expr>,
    pub reason: Option<Expr>,
}

/// `checkpoint "name"`: a named state snapshot for later rollback.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointStmt {
    pub name: String,
}

/// `mutex "name" (every <dur>)? (timeout <dur>)? (hold <dur>)? ({ body })?` or the bare release leaf
/// `mutex release "name"`: a named cross-run exclusive lock. `timeout` bounds the wait-to-acquire,
/// `hold` marks the expected maximum section duration without displacing an active holder, and a
/// `body` block brackets a critical section that releases at its end.
#[derive(Debug, Clone, PartialEq)]
pub struct MutexStmt {
    pub name: String,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
    pub hold: Option<i64>,
    /// true when this is a release leaf (`mutex release "name"`); it takes no other clauses or body.
    pub release: bool,
    /// critical-section body; empty for an acquire-only leaf or a release leaf.
    pub body: Vec<Stmt>,
}

/// `throttle "name" rate <n> per <dur> ...`: a named cross-run rate limiter.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrottleStmt {
    pub name: String,
    pub max_per_window: i64,
    pub window_seconds: i64,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
}

/// `cooldown "name" every <dur>`: a named cross-run cooldown gate.
#[derive(Debug, Clone, PartialEq)]
pub struct CooldownStmt {
    pub name: String,
    pub window_seconds: i64,
}

/// A durable join target. A workflow target selects runs by workflow/correlation; a task target
/// joins the exact detached subflow run carried by a prior `task[T]` binding.
#[derive(Debug, Clone, PartialEq)]
pub enum AwaitTarget {
    Workflow(String),
    Task(String),
}

/// `await workflow "name" …` or `await task_name …`: wait for durable work to reach a terminal
/// state. Task joins identify one exact run rather than scanning every run of a workflow.
#[derive(Debug, Clone, PartialEq)]
pub struct AwaitStmt {
    pub target: AwaitTarget,
    pub key: Option<Expr>,
    pub mode: Option<String>,
    pub timeout: Option<i64>,
}

/// `debounce "name" delay <dur> (key <expr>)?`: a trailing-delay window with external reset.
#[derive(Debug, Clone, PartialEq)]
pub struct DebounceStmt {
    pub name: String,
    pub delay_seconds: i64,
    pub key: Option<Expr>,
}

/// `collect "name" max <n> (timeout <dur>)?`: a timed accumulator.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectStmt {
    pub name: String,
    pub max: i64,
    pub timeout: Option<i64>,
}

/// `barrier "name" count <n> ...`: a multi-run rendezvous.
#[derive(Debug, Clone, PartialEq)]
pub struct BarrierStmt {
    pub name: String,
    pub count: i64,
    pub poll_interval: Option<i64>,
    pub timeout: Option<i64>,
}

/// `circuit_breaker "name" threshold <n> window <dur> cooldown <dur>`: a cross-run failure guard.
#[derive(Debug, Clone, PartialEq)]
pub struct CircuitBreakerStmt {
    pub name: String,
    pub threshold: i64,
    pub window_seconds: i64,
    pub cooldown_seconds: i64,
}

/// `event_source type <str> (filter <cond>)? (max <n>)? (timeout <dur>)?`: stream-driven iteration.
#[derive(Debug, Clone, PartialEq)]
pub struct EventSourceStmt {
    pub event_type: String,
    pub filter: Option<Cond>,
    pub max: Option<i64>,
    pub timeout: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    /// each arm is a (condition, body); the first is `if`, the rest are `else if`.
    pub arms: Vec<(Cond, Block)>,
    pub else_block: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForStmt {
    pub var: String,
    /// optional item annotation used when source inference is too broad.
    pub var_type: Option<TypeExpr>,
    /// optional zero-based loop-position binding.
    pub index_var: Option<String>,
    pub items: Expr,
    /// iteration cap. `None` is uncapped (`limit none` or no clause). a literal
    /// integer lowers to the node's `max_iterations`; any other expression is
    /// carried in the loop parameters and resolved at runtime.
    pub limit: Option<Expr>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhileStmt {
    pub cond: Cond,
    /// `until c` sets this; the loop runs while `!cond`. lowering negates `cond`.
    pub negate: bool,
    pub limit: Option<i64>,
    pub body: Block,
}

/// which router a `match`-family statement lowers to: `switch` cases, a `toggle` on/off, or a
/// `percentage` weighted split. carried on `MatchStmt` so all three reuse the same arm plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchMode {
    Cases,
    Toggle,
    Percentage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchStmt {
    pub subject: Expr,
    pub mode: SwitchMode,
    pub arms: Vec<MatchArm>,
    pub default: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// `Some(expr)` means an equality case; `None` (with `cond`) means a `when` case.
    pub equals: Option<Expr>,
    pub when: Option<Cond>,
    /// percentage-mode weight for this arm (the `N` in `N% -> …`).
    pub weight: Option<i64>,
    /// toggle-mode branch: `Some(true)` is the `on` arm, `Some(false)` the `off` arm.
    pub toggle: Option<bool>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelStmt {
    pub branches: Vec<ParallelBranch>,
    pub join: BranchPolicy,
    /// `None` retains the historical implicit "all branches" join.
    pub selected_branches: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelBranch {
    pub label: Option<String>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryStmt {
    pub body: Block,
    pub catch: Option<Block>,
    pub finally: Option<Block>,
}

/// `resume [next|restart|fail]`. `None` is the bare form: resume at the interrupted node.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeStmt {
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RaceStmt {
    pub branches: Vec<Block>,
    pub winner: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapStmt {
    pub var: String,
    pub items: Expr,
    pub concurrency: Option<i64>,
    pub body: Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPolicy {
    All,
    Any,
    FirstSuccess,
}
