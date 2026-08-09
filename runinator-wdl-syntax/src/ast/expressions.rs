use super::*;

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
    /// an `expr as Type` cast: an author-time type assertion. it is erased at lowering (the runtime
    /// value is the inner expression's, unchanged), but it drives inference so an opaque value —
    /// `parse_json(s)`, an empty `[]` — adopts the annotated shape at that position.
    Cast {
        expr: Box<Expr>,
        ty: TypeExpr,
    },
    /// application of an arbitrary callee value to arguments (`(obj.f)(x)`, `fns[0](x)`). the callee
    /// evaluates to a first-class closure. a bare `name(args)` stays a `Call`; this is only the
    /// field/index/parenthesized-callee form.
    Apply {
        callee: Box<Expr>,
        args: Vec<Expr>,
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

/// the statically-known string keys an expression denotes, used to type key-driven intrinsics
/// (`at`/`pick`/`omit`): a plain string literal yields one key, a literal array of string literals
/// yields several, and anything else (interpolation, a reference, a non-string) yields `None`.
pub fn static_string_keys(expr: &Expr) -> Option<Vec<String>> {
    match &expr.kind {
        ExprKind::Str(parts) => literal_string(parts).map(|key| vec![key]),
        ExprKind::Array(items) => items
            .iter()
            .map(|item| match &item.kind {
                ExprKind::Str(parts) => literal_string(parts),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// the literal value of a string expression's parts, or `None` when it contains interpolation.
fn literal_string(parts: &[StrPart]) -> Option<String> {
    match parts {
        [] => Some(String::new()),
        [StrPart::Lit(text)] => Some(text.clone()),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    Key(String),
    Index(usize),
}
