#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Workflow {
    /// Default portable workspace attached to workflow actions and its completion checkpoint.
    pub workspace: Option<Expr>,
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
    /// portable custom metadata not owned by a structured REXRAP header.
    pub metadata: Option<Expr>,
    /// portable graph-editor layout and presentation data.
    pub ui: Option<Expr>,
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
