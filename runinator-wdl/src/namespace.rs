// namespace resolution runs after parsing and before desugar/sema/lowering. it rewrites every
// call to its bare runtime form so the rest of the pipeline is namespace-free:
//
//   - a qualified prefix call `std.<module>.<leaf>(args)` parses as a fluent method call on the
//     namespace path `std.<module>`; this pass validates the module and drops the receiver, leaving
//     the bare leaf the reducer dispatches on.
//   - an aliased call `s.<leaf>(args)` (from `import std.strings as s`) resolves the alias to its
//     target module and rewrites the same way.
//   - a bare prefix call `foo(args)` is required to be a user function or an imported intrinsic; a
//     bare prefix call to a builtin intrinsic is rejected with guidance to qualify or import it.
//   - a fluent method call on a value (`xs.filter(p)`) and synthetic index access (`at`) keep their
//     bare names — the method syntax is the namespace-free sugar.
//
// std stays a surface concept: the compiled graph and runtime dispatch never see the `std.` prefix.

use std::collections::{HashMap, HashSet};

use runinator_workflows::{STD_MODULES, STD_NAMESPACE, intrinsic_module, is_known_intrinsic};

use crate::ast::*;
use crate::errors::{Span, WdlError};

/// reserved roots that may not be shadowed by an import alias.
const RESERVED_ROOTS: &[&str] = &[STD_NAMESPACE, "params", "prev", "run", "config", "secret"];

/// the per-workflow name scope: imports, the leaves they bring into bare scope, and user functions.
struct Scope {
    /// import alias -> target namespace path (e.g. `s` -> `std.strings`).
    aliases: HashMap<String, String>,
    /// intrinsic leaves callable bare because their std module was imported unaliased.
    bare_intrinsics: HashSet<String>,
    /// user-defined function names (callable bare).
    user_fns: HashSet<String>,
}

impl Scope {
    /// an empty scope for standalone fragments: no imports, no user functions.
    fn empty() -> Self {
        Self {
            aliases: HashMap::new(),
            bare_intrinsics: HashSet::new(),
            user_fns: HashSet::new(),
        }
    }
}

/// resolve a standalone expression fragment (editor/tooling surface; no imports or user functions).
pub(crate) fn resolve_expr_fragment(expr: &mut Expr) -> Result<(), WdlError> {
    resolve_expr(expr, &Scope::empty())
}

/// resolve a standalone condition fragment.
pub(crate) fn resolve_cond_fragment(cond: &mut Cond) -> Result<(), WdlError> {
    resolve_cond(cond, &Scope::empty())
}

/// resolve a standalone compute fragment.
pub(crate) fn resolve_compute_fragment(body: &mut [ComputeLine]) -> Result<(), WdlError> {
    resolve_compute_block(body, &Scope::empty())
}

/// resolve every namespaced call in the document to its bare runtime form, in place.
pub fn resolve(document: &mut Document) -> Result<(), WdlError> {
    let function_scope = build_function_scope(document);
    for function in document.functions.iter_mut() {
        for param in function.params.iter_mut() {
            if let Some(default) = param.default.as_mut() {
                resolve_expr(default, &function_scope)?;
            }
        }
        match &mut function.body {
            FnBody::Expr(expr) => resolve_expr(expr, &function_scope)?,
            FnBody::Block(lines) => resolve_compute_block(lines, &function_scope)?,
        }
    }
    let user_fns = document
        .functions
        .iter()
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();
    for workflow in document.workflows.iter_mut() {
        let scope = build_scope(workflow, user_fns.clone())?;
        for alias in workflow.aliases.iter_mut() {
            for (_, value) in alias.entries.iter_mut() {
                resolve_expr(value, &scope)?;
            }
        }
        for trigger in workflow.triggers.iter_mut() {
            resolve_expr(&mut trigger.schedule, &scope)?;
            if let Some(params) = trigger.params.as_mut() {
                resolve_expr(params, &scope)?;
            }
            if let Some(start) = trigger.blackout_start.as_mut() {
                resolve_expr(start, &scope)?;
            }
            if let Some(end) = trigger.blackout_end.as_mut() {
                resolve_expr(end, &scope)?;
            }
        }
        if let Some(input) = workflow.input.as_mut() {
            resolve_type_defaults(input, &scope)?;
        }
        resolve_block(&mut workflow.body, &scope)?;
    }
    Ok(())
}

/// build the name scope from the document's user functions and `import` declarations.
fn build_function_scope(document: &Document) -> Scope {
    Scope {
        aliases: HashMap::new(),
        bare_intrinsics: HashSet::new(),
        user_fns: document
            .functions
            .iter()
            .map(|function| function.name.clone())
            .collect(),
    }
}

fn build_scope(workflow: &Workflow, user_fns: HashSet<String>) -> Result<Scope, WdlError> {
    let mut aliases = HashMap::new();
    let mut bare_intrinsics = HashSet::new();
    for import in &workflow.imports {
        let segments: Vec<&str> = import.path.split('.').collect();
        // `import std` opens the entire standard library into bare scope; it cannot be aliased.
        if segments.as_slice() == [STD_NAMESPACE] {
            if import.alias.is_some() {
                return Err(WdlError::semantic(
                    import.span,
                    "cannot alias the whole std root; import a specific module (e.g. std.strings)"
                        .to_string(),
                ));
            }
            for leaf in all_intrinsics() {
                bare_intrinsics.insert(leaf);
            }
            continue;
        }
        let is_std = segments.first() == Some(&STD_NAMESPACE);
        if is_std {
            // `import std.<module>` opens a single builtin module.
            let [_, module] = segments.as_slice() else {
                return Err(WdlError::semantic(
                    import.span,
                    format!(
                        "import a specific std module (e.g. std.strings), not '{}'",
                        import.path
                    ),
                ));
            };
            if !STD_MODULES.contains(module) {
                return Err(WdlError::semantic(
                    import.span,
                    format!("unknown std module 'std.{module}'"),
                ));
            }
        }
        match &import.alias {
            Some(alias) => {
                if RESERVED_ROOTS.contains(&alias.as_str()) {
                    return Err(WdlError::semantic(
                        import.span,
                        format!("import alias '{alias}' is reserved"),
                    ));
                }
                if aliases.insert(alias.clone(), import.path.clone()).is_some() {
                    return Err(WdlError::semantic(
                        import.span,
                        format!("duplicate import alias '{alias}'"),
                    ));
                }
            }
            None if is_std => {
                // bring every leaf of the imported module into bare scope.
                let module = segments[1];
                for leaf in intrinsics_in_module(module) {
                    bare_intrinsics.insert(leaf);
                }
            }
            // a bare (un-aliased) non-std import names a workflow namespace used only by subflow
            // resolution; it contributes no bare compute names here.
            None => {}
        }
    }
    Ok(Scope {
        aliases,
        bare_intrinsics,
        user_fns,
    })
}

/// every known intrinsic leaf (pure, effectful, and higher-order).
fn all_intrinsics() -> Vec<String> {
    runinator_workflows::PureIntrinsics::names()
        .iter()
        .chain(runinator_workflows::EFFECTFUL_INTRINSIC_NAMES.iter())
        .chain(runinator_workflows::HIGHER_ORDER_NAMES.iter())
        .map(|leaf| leaf.to_string())
        .collect()
}

/// every intrinsic leaf that belongs to a given std module.
fn intrinsics_in_module(module: &str) -> Vec<String> {
    all_intrinsics()
        .into_iter()
        .filter(|leaf| intrinsic_module(leaf) == Some(module))
        .collect()
}

fn resolve_block(block: &mut Block, scope: &Scope) -> Result<(), WdlError> {
    for stmt in block.iter_mut() {
        resolve_stmt(stmt, scope)?;
    }
    Ok(())
}

fn resolve_stmt(stmt: &mut Stmt, scope: &Scope) -> Result<(), WdlError> {
    match &mut stmt.kind {
        StmtKind::Action(action) => resolve_entries(&mut action.args, scope)?,
        StmtKind::Compute(compute) => resolve_compute_block(&mut compute.body, scope)?,
        StmtKind::Subflow(subflow) => {
            if let Some(run_name) = subflow.run_name.as_mut() {
                resolve_expr(run_name, scope)?;
            }
            resolve_entries(&mut subflow.params, scope)?;
        }
        StmtKind::Approval(approval) => {
            resolve_expr(&mut approval.prompt, scope)?;
            resolve_entries(&mut approval.metadata, scope)?;
        }
        StmtKind::Gate(gate) => {
            if let Some(when) = gate.when.as_mut() {
                resolve_cond(when, scope)?;
            }
            resolve_entries(&mut gate.metadata, scope)?;
        }
        StmtKind::Signal(signal) => resolve_entries(&mut signal.metadata, scope)?,
        StmtKind::Config(config) => {
            if let Some(name) = config.name.as_mut() {
                resolve_expr(name, scope)?;
            }
            if let Some(metadata) = config.metadata.as_mut() {
                resolve_expr(metadata, scope)?;
            }
        }
        StmtKind::Output(output) => {
            if let Some(data) = output.data.as_mut() {
                resolve_expr(data, scope)?;
            }
            for (_, source) in output.items.iter_mut() {
                resolve_expr(source, scope)?;
            }
        }
        StmtKind::Yield(value) => resolve_expr(value, scope)?,
        StmtKind::Input(input) => {
            if let Some(prompt) = input.prompt.as_mut() {
                resolve_expr(prompt, scope)?;
            }
        }
        StmtKind::Wait(wait) => {
            if let WaitAmount::Expr(expr) = &mut wait.amount {
                resolve_expr(expr, scope)?;
            }
        }
        StmtKind::Fail(expr) => {
            if let Some(expr) = expr.as_mut() {
                resolve_expr(expr, scope)?;
            }
        }
        StmtKind::If(if_stmt) => {
            for (cond, body) in if_stmt.arms.iter_mut() {
                resolve_cond(cond, scope)?;
                resolve_block(body, scope)?;
            }
            if let Some(body) = if_stmt.else_block.as_mut() {
                resolve_block(body, scope)?;
            }
        }
        StmtKind::For(for_stmt) => {
            resolve_expr(&mut for_stmt.items, scope)?;
            resolve_block(&mut for_stmt.body, scope)?;
        }
        StmtKind::While(while_stmt) => {
            resolve_cond(&mut while_stmt.cond, scope)?;
            resolve_block(&mut while_stmt.body, scope)?;
        }
        StmtKind::Map(map_stmt) => {
            resolve_expr(&mut map_stmt.items, scope)?;
            resolve_block(&mut map_stmt.body, scope)?;
        }
        StmtKind::Match(match_stmt) => {
            resolve_expr(&mut match_stmt.subject, scope)?;
            for arm in match_stmt.arms.iter_mut() {
                if let Some(equals) = arm.equals.as_mut() {
                    resolve_expr(equals, scope)?;
                }
                if let Some(when) = arm.when.as_mut() {
                    resolve_cond(when, scope)?;
                }
                resolve_block(&mut arm.body, scope)?;
            }
            if let Some(body) = match_stmt.default.as_mut() {
                resolve_block(body, scope)?;
            }
        }
        StmtKind::Parallel(parallel) => {
            for branch in parallel.branches.iter_mut() {
                resolve_block(branch, scope)?;
            }
        }
        StmtKind::Race(race) => {
            for branch in race.branches.iter_mut() {
                resolve_block(branch, scope)?;
            }
        }
        StmtKind::Try(try_stmt) => {
            resolve_block(&mut try_stmt.body, scope)?;
            if let Some(body) = try_stmt.catch.as_mut() {
                resolve_block(body, scope)?;
            }
            if let Some(body) = try_stmt.finally.as_mut() {
                resolve_block(body, scope)?;
            }
        }
        StmtKind::Assert(assert) => {
            for (_, cond) in assert.assertions.iter_mut() {
                resolve_cond(cond, scope)?;
            }
        }
        StmtKind::Transform(transform) => {
            for (_, value) in transform.bindings.iter_mut() {
                resolve_expr(value, scope)?;
            }
        }
        StmtKind::Audit(audit) => {
            resolve_expr(&mut audit.action, scope)?;
            for value in [
                audit.actor.as_mut(),
                audit.target.as_mut(),
                audit.reason.as_mut(),
            ]
            .into_iter()
            .flatten()
            {
                resolve_expr(value, scope)?;
            }
        }
        StmtKind::Await(await_stmt) => resolve_expr(&mut await_stmt.run_ids, scope)?,
        StmtKind::Debounce(debounce) => {
            if let Some(key) = debounce.key.as_mut() {
                resolve_expr(key, scope)?;
            }
        }
        StmtKind::EventSource(es) => {
            if let Some(filter) = es.filter.as_mut() {
                resolve_cond(filter, scope)?;
            }
        }
        StmtKind::Mutex(mutex) => resolve_block(&mut mutex.body, scope)?,
        // no namespace-qualified references to resolve.
        StmtKind::Checkpoint(_)
        | StmtKind::Throttle(_)
        | StmtKind::Collect(_)
        | StmtKind::Barrier(_)
        | StmtKind::CircuitBreaker(_) => {}
    }
    Ok(())
}

fn resolve_compute_block(body: &mut [ComputeLine], scope: &Scope) -> Result<(), WdlError> {
    for line in body.iter_mut() {
        match line {
            ComputeLine::Let { value, .. }
            | ComputeLine::Return(value)
            | ComputeLine::Expr(value) => resolve_expr(value, scope)?,
            ComputeLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                resolve_cond(cond, scope)?;
                resolve_compute_block(then_branch, scope)?;
                resolve_compute_block(else_branch, scope)?;
            }
            ComputeLine::Goto(_) => {}
        }
    }
    Ok(())
}

fn resolve_cond(cond: &mut Cond, scope: &Scope) -> Result<(), WdlError> {
    match &mut cond.kind {
        CondKind::All(conds) | CondKind::Any(conds) => {
            for cond in conds.iter_mut() {
                resolve_cond(cond, scope)?;
            }
        }
        CondKind::Not(inner) => resolve_cond(inner, scope)?,
        CondKind::Expr(expr) => resolve_expr(expr, scope)?,
        CondKind::Cmp { left, right, .. } => {
            resolve_expr(left, scope)?;
            resolve_expr(right, scope)?;
        }
        CondKind::Exists(expr) => resolve_expr(expr, scope)?,
    }
    Ok(())
}

fn resolve_entries(entries: &mut [(String, Expr)], scope: &Scope) -> Result<(), WdlError> {
    for (_, value) in entries.iter_mut() {
        resolve_expr(value, scope)?;
    }
    Ok(())
}

// resolve the optional defaults carried on top-level workflow parameter fields.
fn resolve_type_defaults(ty: &mut TypeExpr, scope: &Scope) -> Result<(), WdlError> {
    match ty {
        TypeExpr::Struct { fields, additional } => {
            for field in fields.iter_mut() {
                if let Some(default) = field.default.as_mut() {
                    resolve_expr(default, scope)?;
                }
                resolve_type_defaults(&mut field.ty, scope)?;
            }
            if let Some(additional) = additional.as_mut() {
                resolve_type_defaults(additional, scope)?;
            }
        }
        TypeExpr::Array(inner) | TypeExpr::Map(inner) => resolve_type_defaults(inner, scope)?,
        TypeExpr::Range { base, .. } => resolve_type_defaults(base, scope)?,
        TypeExpr::Union(variants) => {
            for variant in variants.iter_mut() {
                resolve_type_defaults(variant, scope)?;
            }
        }
        TypeExpr::Named(_) | TypeExpr::Enum(_) => {}
    }
    Ok(())
}

fn resolve_expr(expr: &mut Expr, scope: &Scope) -> Result<(), WdlError> {
    let span = expr.span;
    match &mut expr.kind {
        ExprKind::Call {
            name,
            args,
            named,
            method,
        } => {
            // try to rewrite a namespaced method call (`std.module.leaf(..)` / `alias.leaf(..)`)
            // into a bare call; otherwise enforce the std-qualification rule on prefix calls.
            if *method {
                if let Some(leaf) = namespaced_leaf(name, args.first(), scope, span)? {
                    *name = leaf;
                    args.remove(0);
                    *method = false;
                }
            } else {
                enforce_prefix_call(name, scope, span)?;
            }
            for arg in args.iter_mut() {
                resolve_expr(arg, scope)?;
            }
            for (_, value) in named.iter_mut() {
                resolve_expr(value, scope)?;
            }
        }
        ExprKind::Lambda { body, .. } => resolve_expr(body, scope)?,
        ExprKind::Object(entries) => {
            for (_, value) in entries.iter_mut() {
                resolve_expr(value, scope)?;
            }
        }
        ExprKind::Array(items) => {
            for item in items.iter_mut() {
                resolve_expr(item, scope)?;
            }
        }
        ExprKind::Concat(parts)
        | ExprKind::Coalesce(parts)
        | ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts.iter_mut() {
                resolve_expr(part, scope)?;
            }
        }
        ExprKind::ToString(inner) | ExprKind::ToJson(inner) | ExprKind::Neg(inner) => {
            resolve_expr(inner, scope)?
        }
        ExprKind::Compare { left, right, .. } => {
            resolve_expr(left, scope)?;
            resolve_expr(right, scope)?;
        }
        ExprKind::Ternary { cond, then, els } => {
            resolve_expr(cond, scope)?;
            resolve_expr(then, scope)?;
            resolve_expr(els, scope)?;
        }
        ExprKind::Str(parts) => {
            for part in parts.iter_mut() {
                if let StrPart::Expr(part) = part {
                    resolve_expr(part, scope)?;
                }
            }
        }
        // a namespace used as a value (not called) is an error; a genuine value path is fine.
        ExprKind::Path(segs) => reject_namespace_value(segs, scope, span)?,
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::FileInclude { .. }
        | ExprKind::DirInclude { .. }
        | ExprKind::InlineCode { .. }
        | ExprKind::Spread(_) => {}
    }
    Ok(())
}

// if `receiver` is a namespace path (`std.module` or an import alias), validate that `leaf` names a
// member of it and return the bare leaf to dispatch on. returns `None` for a genuine value receiver
// (an ordinary fluent method call), leaving the call untouched.
fn namespaced_leaf(
    leaf: &str,
    receiver: Option<&Expr>,
    scope: &Scope,
    span: Span,
) -> Result<Option<String>, WdlError> {
    let Some(Expr {
        kind: ExprKind::Path(segs),
        ..
    }) = receiver
    else {
        return Ok(None);
    };
    let keys: Vec<&str> = segs.iter().filter_map(path_key).collect();
    let Some(head) = keys.first().copied() else {
        return Ok(None);
    };
    // a std-qualified call: the receiver must be exactly `std.<module>`.
    if head == STD_NAMESPACE {
        let [_, module] = keys.as_slice() else {
            return Err(WdlError::semantic(
                span,
                "std functions are addressed as std.<module>.<name>".to_string(),
            ));
        };
        return resolve_std_leaf(module, leaf, span).map(Some);
    }
    // an aliased import: the receiver must be exactly the alias.
    if let Some(target) = scope.aliases.get(head) {
        if keys.len() != 1 {
            return Ok(None);
        }
        let target_segs: Vec<&str> = target.split('.').collect();
        return match target_segs.as_slice() {
            [ns, module] if *ns == STD_NAMESPACE => resolve_std_leaf(module, leaf, span).map(Some),
            // a workflow-namespace alias has no callable members in compute.
            _ => Err(WdlError::semantic(
                span,
                format!("namespace '{head}' ({target}) has no function '{leaf}'"),
            )),
        };
    }
    Ok(None)
}

// resolve `std.<module>.<leaf>` to the bare leaf, with a precise error when the module is wrong.
fn resolve_std_leaf(module: &str, leaf: &str, span: Span) -> Result<String, WdlError> {
    match runinator_workflows::resolve_std_path(module, leaf) {
        Ok(_) => Ok(leaf.to_string()),
        Err(Some(actual)) => Err(WdlError::semantic(
            span,
            format!("no function '{leaf}' in std.{module}; it lives in std.{actual}"),
        )),
        Err(None) => Err(WdlError::semantic(
            span,
            format!("'std.{module}.{leaf}' is not a builtin function"),
        )),
    }
}

// a bare prefix call must be a user function or an imported intrinsic; a bare prefix call to a
// builtin intrinsic is rejected with guidance to qualify or import it.
fn enforce_prefix_call(name: &str, scope: &Scope, span: Span) -> Result<(), WdlError> {
    if scope.user_fns.contains(name) || scope.bare_intrinsics.contains(name) {
        return Ok(());
    }
    if is_known_intrinsic(name) {
        let hint = match intrinsic_module(name) {
            Some(module) => format!("std.{module}.{name}(...) or `import std.{module}`"),
            None => format!("std.<module>.{name}(...)"),
        };
        return Err(WdlError::semantic(
            span,
            format!("'{name}' is a builtin intrinsic and must be qualified: use {hint}"),
        ));
    }
    // an unknown bare name (likely a user-function typo) is left for sema to report.
    Ok(())
}

// reject a namespace path used as a value (e.g. `std.strings` or an import alias on its own).
fn reject_namespace_value(segs: &[PathSeg], scope: &Scope, span: Span) -> Result<(), WdlError> {
    let Some(head) = segs.first().and_then(|seg| path_key(seg)) else {
        return Ok(());
    };
    if head == STD_NAMESPACE {
        return Err(WdlError::semantic(
            span,
            "'std' is a namespace and cannot be used as a value".to_string(),
        ));
    }
    if scope.aliases.contains_key(head) {
        return Err(WdlError::semantic(
            span,
            format!("'{head}' is an imported namespace and cannot be used as a value"),
        ));
    }
    Ok(())
}

fn path_key(seg: &PathSeg) -> Option<&str> {
    match seg {
        PathSeg::Key(key) => Some(key.as_str()),
        PathSeg::Index(_) => None,
    }
}
