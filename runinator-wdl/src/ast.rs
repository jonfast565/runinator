// the wdl abstract syntax tree. mirrors the surface grammar in wdl.pest and is the
// single input to lowering. it intentionally stays free of runinator-models types so
// the grammar can evolve independently of the json wire model.

use crate::errors::Span;
use runinator_models::semver::SemVer;

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// top-level `fn` definitions, callable from the workflow body, compute blocks, and other
    /// function bodies. siblings of the workflow.
    pub functions: Vec<FunctionDef>,
    pub workflow: Workflow,
}

/// a top-level `fn name(params) -> ret = body` definition. the body is either a single expression
/// or a compute-style statement block; `recursive` carries the `@recursive(max_depth: N)` cap when
/// present.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<FnParam>,
    pub ret: Option<TypeExpr>,
    pub body: FnBody,
    pub recursive: Option<u32>,
    pub span: Span,
}

/// a function body: a single expression (`= expr`) or a compute-style block of statements
/// (`= { let …; … ; return e }`). block bodies reuse the compute-line grammar and lower to the same
/// `$let`/`$return`/`$if` program form a `compute` block produces.
#[derive(Debug, Clone, PartialEq)]
pub enum FnBody {
    Expr(Box<Expr>),
    Block(Vec<ComputeLine>),
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
    pub version: Option<SemVer>,
    /// top-level workflow parameters, surfaced in source as `params { ... }`.
    pub input: Option<TypeExpr>,
    /// header `alias <name> = { ... }` declarations; reusable argument groups expanded into
    /// action calls by `...name` spreads during desugaring.
    pub aliases: Vec<Alias>,
    /// optional header `namespace <path>` declaration: the namespace this workflow's identity lives
    /// in. when `None` the importer stamps the pack name.
    pub namespace: Option<String>,
    /// header `import <path> (as <alias>)?` declarations opening namespaces into local scope.
    pub imports: Vec<Import>,
    /// an optional explicit `start -> <target>` entry edge. when `None` the first body
    /// statement is the entry; when set it names the entry node directly.
    pub start: Option<Target>,
    /// header `trigger cron "..."` declarations scheduling runs of this workflow.
    pub triggers: Vec<TriggerDecl>,
    /// header `watch <cond> -> <target>` cancellation guards, evaluated on every reducer drive.
    pub watches: Vec<WatchDecl>,
    /// header `type <Name> ...` declarations: reusable named types.
    pub type_decls: Vec<TypeDecl>,
    pub body: Block,
    pub span: Span,
}

/// a header `type <Name> { ... }` (struct shorthand) or `type <Name> = <type>` (alias) declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

/// a header `trigger cron <schedule> (with <params>)?` declaration. `schedule` is a string
/// expression (the cron expression); `params` is the optional run parameter object.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerDecl {
    pub schedule: Expr,
    pub params: Option<Expr>,
    pub enabled: bool,
    pub blackout_start: Option<Expr>,
    pub blackout_end: Option<Expr>,
    pub span: Span,
}

/// a header `watch <cond> -> <target>` guard: when `cond` holds, the run jumps to `handler`.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchDecl {
    pub cond: Cond,
    pub handler: Target,
}

/// a header `import <path> (as <alias>)?` declaration. `path` is the dotted namespace
/// (`std.strings`, `some_pack`); `alias` binds a short local name when present.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub path: String,
    pub alias: Option<String>,
    pub span: Span,
}

/// a header `alias <name> = { k: expr, ... }` binding: a named, reusable group of argument
/// values spread into action calls with `...name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    pub name: String,
    pub entries: Vec<(String, Expr)>,
    pub span: Span,
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub span: Span,
    pub annotations: Annotations,
    /// `let <label> = ...`; the binding doubles as the generated node id.
    pub label: Option<String>,
    /// an optional `let <label>: <type> = ...` annotation declaring the step's output type.
    pub label_type: Option<TypeExpr>,
    pub kind: StmtKind,
    pub transitions: TransitionClause,
    /// `compensate <call>` on an action node: the compensating action run in reverse on saga rollback.
    pub compensation: Option<Box<ActionStmt>>,
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
    Compute(ComputeStmt),
    Subflow(SubflowStmt),
    Wait(WaitStmt),
    Output(OutputStmt),
    Deliverable(DeliverableStmt),
    Input(InputStmt),
    Approval(ApprovalStmt),
    Gate(GateStmt),
    Signal(SignalStmt),
    Config(ConfigStmt),
    Fail(Option<Expr>),
    If(IfStmt),
    For(ForStmt),
    While(WhileStmt),
    Match(MatchStmt),
    Parallel(ParallelStmt),
    Try(TryStmt),
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
    Done,
    Fail,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Modifiers {
    pub timeout_seconds: Option<i64>,
    pub retry: Option<RetryConfig>,
    pub tags: Vec<String>,
    pub mcp: bool,
    pub reentry: Option<Reentry>,
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

/// an imperative `compute { ... }` block. lowers to a `std.run`/`std.exec` action node.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeStmt {
    pub body: Vec<ComputeLine>,
    pub modifiers: Modifiers,
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
    /// `spawn` / `detached` => fire-and-forget; `call` => wait.
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputStmt {
    pub prompt: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeliverableStmt {
    /// (name, artifact-valued expression) pairs in source order.
    pub items: Vec<(String, Expr)>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    /// each arm is a (condition, body); the first is `if`, the rest are `else if`.
    pub arms: Vec<(Cond, Block)>,
    pub else_block: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForStmt {
    pub var: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct MatchStmt {
    pub subject: Expr,
    pub arms: Vec<MatchArm>,
    pub default: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// `Some(expr)` means an equality case; `None` (with `cond`) means a `when` case.
    pub equals: Option<Expr>,
    pub when: Option<Cond>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelStmt {
    pub branches: Vec<Block>,
    pub join: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryStmt {
    pub body: Block,
    pub catch: Option<Block>,
    pub finally: Option<Block>,
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

// expressions ---------------------------------------------------------------

/// an expression paired with the source span it was parsed from, so diagnostics can
/// anchor to the offending sub-expression rather than the enclosing statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// a string literal, possibly with `${...}` interpolations.
    Str(Vec<StrPart>),
    /// a compile-time text include, resolved relative to the source file's directory.
    FileInclude {
        path: String,
    },
    /// a compile-time directory listing, resolved relative to the source file's directory. lowers
    /// to an array of the relative file paths found under `path`. `recursive` descends into
    /// subdirectories; `max_depth` caps how many levels are walked (`None` is unlimited).
    DirInclude {
        path: String,
        recursive: bool,
        max_depth: Option<usize>,
    },
    /// a fenced source block that lowers to its literal text.
    InlineCode {
        language: String,
        content: String,
    },
    /// a dotted reference: `params.a.b`, `prev.x`, `run.id`, `<binding>.field`.
    Path(Vec<PathSeg>),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    /// `a ++ b` string concatenation.
    Concat(Vec<Expr>),
    /// `a ?? b` first-non-null.
    Coalesce(Vec<Expr>),
    /// `string(x)` coercion.
    ToString(Box<Expr>),
    /// `json(x)` serialization.
    ToJson(Box<Expr>),
    /// a `...alias` spread placeholder, only valid as an argument or object entry value. expanded
    /// away by desugaring; never reaches sema or lowering. the carried name is the alias.
    Spread(String),
    // compute-tier arithmetic; only produced inside `compute { }` blocks.
    Add(Vec<Expr>),
    Sub(Vec<Expr>),
    Mul(Vec<Expr>),
    Div(Vec<Expr>),
    Mod(Vec<Expr>),
    Neg(Box<Expr>),
    /// a relational comparison `left <op> right`, lowering to the matching pure intrinsic
    /// (`==`→`eq`, `!=`→`ne`, `<`→`lt`, `<=`→`lte`, `>`→`gt`, `>=`→`gte`). resolves to a boolean.
    Compare {
        op: CompareOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// a lazy conditional `cond ? then : els`, lowering to the runtime `$if` form. only the taken
    /// branch is evaluated, so a recursive function's base case terminates.
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// a library or user-function call `name(args...)`, e.g. `add(a, b)` or `double(x)`. positional
    /// arguments are in `args`; trailing keyword arguments (`f(x, k: v)`) are in `named`. the
    /// lowering pass resolves `named` into positional order against the callee's signature.
    ///
    /// `method` records the syntactic origin so namespace resolution can require qualification of
    /// prefix intrinsic calls (`std.math.add(a, b)`) while leaving fluent method calls
    /// (`xs.filter(p)`, which desugar to `filter(xs, p)`) and synthetic index access (`at`) as
    /// sugar. it is set during parsing and ignored by sema and lowering.
    Call {
        name: String,
        args: Vec<Expr>,
        named: Vec<(String, Expr)>,
        method: bool,
    },
    /// an anonymous function `params => body`, only valid inside `compute { }` as the argument to a
    /// higher-order library call (`map`, `filter`, `reduce`, ...).
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
}

/// the relational operators available at expression level, each backed by a pure intrinsic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
}

impl CompareOp {
    /// the pure intrinsic this operator lowers to.
    pub fn intrinsic(self) -> &'static str {
        match self {
            CompareOp::Eq => "eq",
            CompareOp::Ne => "ne",
            CompareOp::Lt => "lt",
            CompareOp::Lte => "lte",
            CompareOp::Gt => "gt",
            CompareOp::Gte => "gte",
        }
    }

    /// the source token, used by the formatter.
    pub fn token(self) -> &'static str {
        match self {
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
            CompareOp::Lt => "<",
            CompareOp::Lte => "<=",
            CompareOp::Gt => ">",
            CompareOp::Gte => ">=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    Key(String),
    Index(usize),
}

// conditions ----------------------------------------------------------------

/// a condition paired with the source span it was parsed from.
#[derive(Debug, Clone, PartialEq)]
pub struct Cond {
    pub kind: CondKind,
    pub span: Span,
}

impl Cond {
    pub fn new(kind: CondKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CondKind {
    All(Vec<Cond>),
    Any(Vec<Cond>),
    Not(Box<Cond>),
    Expr(Expr),
    Cmp { left: Expr, op: CmpOp, right: Expr },
    Exists(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    In,
    StartsWith,
    EndsWith,
}

// input types ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Array(Box<TypeExpr>),
    Map(Box<TypeExpr>),
    Struct {
        fields: Vec<TypeField>,
        additional: Option<Box<TypeExpr>>,
    },
    Union(Vec<TypeExpr>),
}

// secrets (.wdls) -----------------------------------------------------------

/// a single `.wdls` declaration: `secret|config <scope>.<name…> = <literal>`. the value must be a
/// pure literal; lowering rejects references and interpolation.
#[derive(Debug, Clone, PartialEq)]
pub struct SecretDecl {
    pub is_config: bool,
    pub path: Vec<PathSeg>,
    pub value: Expr,
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
}
