// the wdl abstract syntax tree. mirrors the surface grammar in wdl.pest and is the
// single input to lowering. it intentionally stays free of runinator-models types so
// the grammar can evolve independently of the json wire model.

use crate::errors::Span;
use runinator_models::semver::SemVer;

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub workflow: Workflow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workflow {
    pub name: String,
    pub version: Option<SemVer>,
    pub input: Option<TypeExpr>,
    /// header `alias <name> = { ... }` declarations; reusable argument groups expanded into
    /// action calls by `...name` spreads during desugaring.
    pub aliases: Vec<Alias>,
    /// an optional explicit `start -> <target>` entry edge. when `None` the first body
    /// statement is the entry; when set it names the entry node directly.
    pub start: Option<Target>,
    /// header `trigger cron "..."` declarations scheduling runs of this workflow.
    pub triggers: Vec<TriggerDecl>,
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
/// expression (the cron expression); `params` is the optional run input object.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerDecl {
    pub schedule: Expr,
    pub params: Option<Expr>,
    pub enabled: bool,
    pub blackout_start: Option<Expr>,
    pub blackout_end: Option<Expr>,
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
    Emit(EmitStmt),
    Approval(ApprovalStmt),
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
}

impl TransitionClause {
    pub fn is_empty(&self) -> bool {
        self.next.is_none()
            && self.on_success.is_none()
            && self.on_failure.is_none()
            && self.on_timeout.is_none()
            && self.on_reject.is_none()
    }
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
    pub retry: Option<i64>,
    pub tags: Vec<String>,
    pub mcp: bool,
    pub reentry: Option<Reentry>,
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
pub struct EmitStmt {
    pub event_type: Option<String>,
    pub data: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalStmt {
    pub approval_type: Option<String>,
    pub prompt: Expr,
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
    pub limit: Option<i64>,
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
    /// a dotted reference: `input.a.b`, `prev.x`, `run.id`, `<binding>.field`.
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
    /// a library call `name(args...)`, e.g. `add(a, b)` or `http_get(url)`.
    Call {
        name: String,
        args: Vec<Expr>,
    },
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
    /// an optional default expression, only present on top-level `input { }` fields. when set the
    /// field is effectively optional and the expression fills it at run start if omitted.
    pub default: Option<Expr>,
}
