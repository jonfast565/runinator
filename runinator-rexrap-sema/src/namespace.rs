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

use runinator_compute::{STD_MODULES, STD_NAMESPACE, intrinsic_module, is_known_intrinsic};

use runinator_rexrap_syntax::ast::*;
use runinator_rexrap_syntax::errors::{RexRapError, Span};

/// reserved roots that may not be shadowed by an import alias.
const RESERVED_ROOTS: &[&str] = &[
    STD_NAMESPACE,
    "params",
    "prev",
    "run",
    "config",
    "secret",
    "interrupt",
];

/// the per-workflow name scope: imports, the leaves they bring into bare scope, and user functions.
struct Scope {
    /// import alias -> target namespace path (e.g. `s` -> `std.strings`).
    aliases: HashMap<String, String>,
    /// intrinsic leaves callable bare because their std module was imported unaliased.
    bare_intrinsics: HashSet<String>,
    /// user-defined function names (callable bare).
    user_fns: HashSet<String>,
    /// typed workflow import alias -> durable-path selector. This remains source-only until the
    /// pack importer resolves it to an ArtifactRef UUID/digest.
    workflow_aliases: HashMap<String, WorkflowImport>,
    /// typed function-package import alias -> the package's authoring path. An action written as
    /// `pdf.render(...)` becomes `functions.acme.shared.pdf.render(...)` before lowering, where
    /// the normal function catalog resolver records the exact package/version/export binding.
    function_aliases: HashMap<String, String>,
    /// typed settings import alias -> durable authoring namespace. `shared.timeout` is config by
    /// default; `shared.secret.token` selects the late-resolved secret family explicitly.
    settings_aliases: HashMap<String, String>,
    /// source-module import alias -> exported leaf -> deterministic embedded function name.
    module_aliases: HashMap<String, HashMap<String, String>>,
    /// bare calls rewritten while preparing one module's private function namespace.
    function_renames: HashMap<String, String>,
    strict_resources: bool,
}

#[derive(Clone)]
struct WorkflowImport {
    path: String,
    revision: Option<i64>,
}

impl Scope {
    /// an empty scope for standalone fragments: no imports, no user functions.
    fn empty() -> Self {
        Self {
            aliases: HashMap::new(),
            bare_intrinsics: HashSet::new(),
            user_fns: HashSet::new(),
            workflow_aliases: HashMap::new(),
            function_aliases: HashMap::new(),
            settings_aliases: HashMap::new(),
            module_aliases: HashMap::new(),
            function_renames: HashMap::new(),
            strict_resources: false,
        }
    }
}

/// resolve a standalone expression fragment (editor/tooling surface; no imports or user functions).
pub fn resolve_expr_fragment(expr: &mut Expr) -> Result<(), RexRapError> {
    resolve_expr(expr, &Scope::empty())
}

/// resolve a standalone condition fragment.
pub fn resolve_cond_fragment(cond: &mut Cond) -> Result<(), RexRapError> {
    resolve_cond(cond, &Scope::empty())
}

/// resolve a standalone compute fragment.
pub fn resolve_compute_fragment(body: &mut [ComputeLine]) -> Result<(), RexRapError> {
    resolve_compute_block(body, &Scope::empty())
}

/// resolve every namespaced call in the document to its bare runtime form, in place.
pub fn resolve(document: &mut Document) -> Result<(), RexRapError> {
    resolve_with_policy(document, false)
}

/// Resolve a pack source under the namespaced-artifact contract. Durable references must use a
/// typed import and every workflow must declare its stable key and namespace.
pub fn resolve_strict(document: &mut Document) -> Result<(), RexRapError> {
    resolve_with_policy(document, true)
}

fn resolve_with_policy(document: &mut Document, strict_resources: bool) -> Result<(), RexRapError> {
    let modules = prepare_source_modules(document)?;
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
            // a `task fn` body is a statement region and resolves like a workflow body.
            FnBody::Run(body) => {
                for stmt in body.iter_mut() {
                    resolve_stmt(stmt, &function_scope)?;
                }
            }
        }
    }
    let user_fns = document
        .functions
        .iter()
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();
    for workflow in document.workflows.iter_mut() {
        if strict_resources && workflow.key.is_none() {
            return Err(RexRapError::semantic(
                workflow.span,
                format!("workflow '{}' must declare a stable `key`", workflow.name),
            ));
        }
        if strict_resources && workflow.namespace.is_none() {
            return Err(RexRapError::semantic(
                workflow.span,
                format!("workflow '{}' must declare a `namespace`", workflow.name),
            ));
        }
        let scope = build_scope(workflow, user_fns.clone(), &modules, strict_resources)?;
        for alias in workflow.aliases.iter_mut() {
            for (_, value) in alias.entries.iter_mut() {
                resolve_expr(value, &scope)?;
            }
        }
        for trigger in workflow.triggers.iter_mut() {
            match &mut trigger.kind {
                TriggerDeclKind::Cron {
                    schedule,
                    blackout_start,
                    blackout_end,
                    ..
                } => {
                    resolve_expr(schedule, &scope)?;
                    if let Some(start) = blackout_start.as_mut() {
                        resolve_expr(start, &scope)?;
                    }
                    if let Some(end) = blackout_end.as_mut() {
                        resolve_expr(end, &scope)?;
                    }
                }
                TriggerDeclKind::Chained { target, .. } => {
                    resolve_expr(target, &scope)?;
                }
            }
            if let Some(params) = trigger.params.as_mut() {
                resolve_expr(params, &scope)?;
            }
        }
        if let Some(correlation) = workflow.correlation.as_mut() {
            resolve_expr(correlation, &scope)?;
        }
        if let Some(input) = workflow.input.as_mut() {
            resolve_type_defaults(input, &scope)?;
        }
        for interrupt in &mut workflow.interrupts {
            resolve_block(&mut interrupt.body, &scope)?;
        }
        resolve_block(&mut workflow.body, &scope)?;
        for join in workflow.joins.iter_mut() {
            resolve_block(&mut join.body, &scope)?;
        }
    }
    Ok(())
}

type ModuleRegistry = HashMap<String, HashMap<String, String>>;

fn prepare_source_modules(document: &mut Document) -> Result<ModuleRegistry, RexRapError> {
    let mut registry = ModuleRegistry::new();
    let mut embedded = Vec::new();
    for module in &document.modules {
        if registry.contains_key(&module.path) {
            return Err(RexRapError::semantic(
                module.span,
                format!("duplicate source module '{}'", module.path),
            ));
        }
        let mut exports = HashMap::new();
        for function in &module.functions {
            let embedded_name = module_function_name(&module.path, &function.name);
            if exports
                .insert(function.name.clone(), embedded_name)
                .is_some()
            {
                return Err(RexRapError::semantic(
                    function.span,
                    format!(
                        "source module '{}' exports function '{}' twice",
                        module.path, function.name
                    ),
                ));
            }
        }
        let scope = Scope {
            aliases: HashMap::new(),
            bare_intrinsics: HashSet::new(),
            user_fns: exports.keys().cloned().collect(),
            workflow_aliases: HashMap::new(),
            function_aliases: HashMap::new(),
            settings_aliases: HashMap::new(),
            module_aliases: HashMap::new(),
            function_renames: exports.clone(),
            strict_resources: false,
        };
        for mut function in module.functions.clone() {
            for param in &mut function.params {
                if let Some(default) = param.default.as_mut() {
                    resolve_expr(default, &scope)?;
                }
            }
            match &mut function.body {
                FnBody::Expr(expr) => resolve_expr(expr, &scope)?,
                FnBody::Block(lines) => resolve_compute_block(lines, &scope)?,
                FnBody::Run(_) => unreachable!("task functions are rejected in source modules"),
            }
            function.name = exports[&function.name].clone();
            embedded.push(function);
        }
        registry.insert(module.path.clone(), exports);
    }
    document.functions.extend(embedded);
    Ok(registry)
}

fn module_function_name(module: &str, function: &str) -> String {
    let encoded = module
        .split('.')
        .map(|segment| format!("{}{}", segment.len(), segment))
        .collect::<Vec<_>>()
        .join("_");
    format!("__module_{encoded}__{function}")
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
        workflow_aliases: HashMap::new(),
        function_aliases: HashMap::new(),
        settings_aliases: HashMap::new(),
        module_aliases: HashMap::new(),
        function_renames: HashMap::new(),
        strict_resources: false,
    }
}

fn build_scope(
    workflow: &Workflow,
    user_fns: HashSet<String>,
    modules: &ModuleRegistry,
    strict_resources: bool,
) -> Result<Scope, RexRapError> {
    let mut aliases = HashMap::new();
    let mut bare_intrinsics = HashSet::new();
    let mut workflow_aliases = HashMap::new();
    let mut function_aliases = HashMap::new();
    let mut settings_aliases = HashMap::new();
    let mut module_aliases = HashMap::new();
    // Every alias shares one workflow-local namespace, even when its consumer is a different
    // resolver (a subflow, a package action, or a source/settings declaration). Letting two kinds
    // reuse a spelling would make a later language feature silently change what old source means.
    let mut claimed_aliases = HashSet::new();
    for import in &workflow.imports {
        if import.kind == Some(ImportKind::Workflow) {
            let alias = import.alias.as_ref().ok_or_else(|| {
                RexRapError::semantic(
                    import.span,
                    format!("workflow import '{}' requires an alias", import.path),
                )
            })?;
            if RESERVED_ROOTS.contains(&alias.as_str()) {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("import alias '{alias}' is reserved"),
                ));
            }
            if !claimed_aliases.insert(alias.clone()) {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("duplicate import alias '{alias}'"),
                ));
            }
            if workflow_aliases
                .insert(
                    alias.clone(),
                    WorkflowImport {
                        path: import.path.clone(),
                        revision: import.revision,
                    },
                )
                .is_some()
            {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("duplicate import alias '{alias}'"),
                ));
            }
            continue;
        }
        if import.kind == Some(ImportKind::Functions) {
            let alias = import.alias.as_ref().ok_or_else(|| {
                RexRapError::semantic(
                    import.span,
                    format!("functions import '{}' requires an alias", import.path),
                )
            })?;
            if RESERVED_ROOTS.contains(&alias.as_str()) {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("import alias '{alias}' is reserved"),
                ));
            }
            if !claimed_aliases.insert(alias.clone()) {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("duplicate import alias '{alias}'"),
                ));
            }
            function_aliases.insert(alias.clone(), import.path.clone());
            continue;
        }
        if matches!(import.kind, Some(ImportKind::Settings | ImportKind::Module)) {
            let alias = import.alias.as_ref().ok_or_else(|| {
                RexRapError::semantic(
                    import.span,
                    format!(
                        "{} import '{}' requires an alias",
                        import.kind.unwrap().keyword(),
                        import.path
                    ),
                )
            })?;
            if RESERVED_ROOTS.contains(&alias.as_str()) {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("import alias '{alias}' is reserved"),
                ));
            }
            if !claimed_aliases.insert(alias.clone()) {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("duplicate import alias '{alias}'"),
                ));
            }
            if import.kind == Some(ImportKind::Settings) {
                settings_aliases.insert(alias.clone(), import.path.clone());
            } else {
                let exports = modules.get(&import.path).ok_or_else(|| {
                    RexRapError::semantic(
                        import.span,
                        format!("pack has no source module '{}'", import.path),
                    )
                })?;
                module_aliases.insert(alias.clone(), exports.clone());
            }
            continue;
        }
        let segments: Vec<&str> = import.path.split('.').collect();
        if strict_resources && segments.first() != Some(&STD_NAMESPACE) {
            return Err(RexRapError::semantic(
                import.span,
                format!(
                    "durable import '{}' must declare one of: workflow, functions, settings, module",
                    import.path
                ),
            ));
        }
        // `import std` opens the entire standard library into bare scope; it cannot be aliased.
        if segments.as_slice() == [STD_NAMESPACE] {
            if import.alias.is_some() {
                return Err(RexRapError::semantic(
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
                return Err(RexRapError::semantic(
                    import.span,
                    format!(
                        "import a specific std module (e.g. std.strings), not '{}'",
                        import.path
                    ),
                ));
            };
            if !STD_MODULES.contains(module) {
                return Err(RexRapError::semantic(
                    import.span,
                    format!("unknown std module 'std.{module}'"),
                ));
            }
        }
        match &import.alias {
            Some(alias) => {
                if RESERVED_ROOTS.contains(&alias.as_str()) {
                    return Err(RexRapError::semantic(
                        import.span,
                        format!("import alias '{alias}' is reserved"),
                    ));
                }
                if !claimed_aliases.insert(alias.clone()) {
                    return Err(RexRapError::semantic(
                        import.span,
                        format!("duplicate import alias '{alias}'"),
                    ));
                }
                aliases.insert(alias.clone(), import.path.clone());
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
        workflow_aliases,
        function_aliases,
        settings_aliases,
        module_aliases,
        function_renames: HashMap::new(),
        strict_resources,
    })
}

/// every known intrinsic leaf (pure, effectful, and higher-order).
fn all_intrinsics() -> Vec<String> {
    runinator_compute::PureIntrinsics::names()
        .iter()
        .chain(runinator_compute::EFFECTFUL_INTRINSIC_NAMES.iter())
        .chain(runinator_compute::HIGHER_ORDER_NAMES.iter())
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

fn resolve_block(block: &mut Block, scope: &Scope) -> Result<(), RexRapError> {
    for stmt in block.iter_mut() {
        resolve_stmt(stmt, scope)?;
    }
    Ok(())
}

fn resolve_stmt(stmt: &mut Stmt, scope: &Scope) -> Result<(), RexRapError> {
    let stmt_span = stmt.span;
    match &mut stmt.kind {
        StmtKind::Action(action) => resolve_action(action, scope, stmt_span)?,
        StmtKind::TaskCall(call) => resolve_entries(&mut call.args, scope)?,
        StmtKind::Return(Some(value)) => resolve_expr(value, scope)?,
        StmtKind::Return(None) | StmtKind::Detach(_) => {}
        StmtKind::Compute(compute) => resolve_compute_block(&mut compute.body, scope)?,
        StmtKind::Subflow(subflow) => {
            if let Some(import) = scope.workflow_aliases.get(&subflow.workflow_name) {
                subflow.workflow_name = import.path.clone();
                subflow.revision = import.revision;
                subflow.imported = true;
            }
            if scope.strict_resources && !subflow.imported {
                return Err(RexRapError::semantic(
                    stmt.span,
                    format!(
                        "workflow '{}' must be referenced through a typed `import workflow ... as ...` alias",
                        subflow.workflow_name
                    ),
                ));
            }
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
            if let Some(limit) = for_stmt.limit.as_mut() {
                resolve_expr(limit, scope)?;
            }
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
                resolve_block(&mut branch.body, scope)?;
            }
        }
        StmtKind::Race(race) => {
            for branch in race.branches.iter_mut() {
                resolve_block(branch, scope)?;
            }
        }
        // `resume` carries no expressions, names, or bindings, so every pass is a no-op.
        StmtKind::Resume(_) => {}
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
        StmtKind::Await(await_stmt) => {
            if let Some(key) = await_stmt.key.as_mut() {
                resolve_expr(key, scope)?;
            }
        }
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
        | StmtKind::Cooldown(_)
        | StmtKind::Collect(_)
        | StmtKind::Barrier(_)
        | StmtKind::CircuitBreaker(_) => {}
    }
    if let Some(compensation) = stmt.compensation.as_mut() {
        resolve_action(compensation, scope, stmt_span)?;
    }
    Ok(())
}

fn resolve_action(action: &mut ActionStmt, scope: &Scope, span: Span) -> Result<(), RexRapError> {
    if scope.strict_resources && action.provider.starts_with("functions.") {
        return Err(RexRapError::semantic(
            span,
            "packaged functions must be referenced through a typed `import functions ... as ...` alias",
        ));
    }
    if let Some(path) = scope.function_aliases.get(&action.provider) {
        action.provider = format!("functions.{path}");
    }
    resolve_entries(&mut action.args, scope)
}

fn resolve_compute_block(body: &mut [ComputeLine], scope: &Scope) -> Result<(), RexRapError> {
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

fn resolve_cond(cond: &mut Cond, scope: &Scope) -> Result<(), RexRapError> {
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

fn resolve_entries(entries: &mut [(String, Expr)], scope: &Scope) -> Result<(), RexRapError> {
    for (_, value) in entries.iter_mut() {
        resolve_expr(value, scope)?;
    }
    Ok(())
}

// resolve the optional defaults carried on top-level workflow parameter fields.
fn resolve_type_defaults(ty: &mut TypeExpr, scope: &Scope) -> Result<(), RexRapError> {
    match ty {
        TypeExpr::Task(Some(inner)) => resolve_type_defaults(inner, scope)?,
        TypeExpr::Task(None) => {}
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
        TypeExpr::Function { params, ret } => {
            for param in params.iter_mut() {
                resolve_type_defaults(param, scope)?;
            }
            resolve_type_defaults(ret, scope)?;
        }
        TypeExpr::Named(_) | TypeExpr::Enum(_) => {}
    }
    Ok(())
}

fn resolve_expr(expr: &mut Expr, scope: &Scope) -> Result<(), RexRapError> {
    let span = expr.span;
    match &mut expr.kind {
        ExprKind::Call {
            name,
            args,
            named,
            method,
            policy,
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
            // the policy object holds ordinary expressions (an idempotency key is usually a ref),
            // so it resolves like any other operand.
            if let Some(policy) = policy {
                resolve_expr(policy, scope)?;
            }
        }
        ExprKind::Lambda { body, .. } => resolve_expr(body, scope)?,
        ExprKind::Cast { expr, .. } => resolve_expr(expr, scope)?,
        ExprKind::Apply { callee, args } => {
            resolve_expr(callee, scope)?;
            for arg in args.iter_mut() {
                resolve_expr(arg, scope)?;
            }
        }
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
        ExprKind::Path(segs) => {
            let settings_alias = resolve_settings_path(segs, scope, span)?;
            if scope.strict_resources
                && !settings_alias
                && matches!(segs.first().and_then(path_key), Some("config" | "secret"))
            {
                return Err(RexRapError::semantic(
                    span,
                    "settings must be referenced through a typed `import settings ... as ...` alias",
                ));
            }
            reject_namespace_value(segs, scope, span)?;
        }
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

/// Rewrite a typed settings alias into the runtime's existing config/secret address shape while
/// retaining the dotted imported namespace as one scope key. Examples:
///
/// `shared.timeout` -> `config.<acme.shared>.timeout`
/// `shared.config.timeout` -> `config.<acme.shared>.timeout`
/// `shared.secret.token` -> `secret.<acme.shared>.token`
fn resolve_settings_path(
    segs: &mut Vec<PathSeg>,
    scope: &Scope,
    span: Span,
) -> Result<bool, RexRapError> {
    let Some(alias) = segs.first().and_then(path_key) else {
        return Ok(false);
    };
    let Some(namespace) = scope.settings_aliases.get(alias) else {
        return Ok(false);
    };
    let mut tail = segs[1..].to_vec();
    let family = match tail.first().and_then(path_key) {
        Some("config") => {
            tail.remove(0);
            "config"
        }
        Some("secret") => {
            tail.remove(0);
            "secret"
        }
        _ => "config",
    };
    if tail.is_empty() {
        return Err(RexRapError::semantic(
            span,
            format!("settings alias '{alias}' requires a setting key"),
        ));
    }
    *segs = vec![
        PathSeg::Key(family.to_string()),
        PathSeg::Key(namespace.clone()),
    ];
    segs.extend(tail);
    Ok(true)
}

// if `receiver` is a namespace path (`std.module` or an import alias), validate that `leaf` names a
// member of it and return the bare leaf to dispatch on. returns `None` for a genuine value receiver
// (an ordinary fluent method call), leaving the call untouched.
fn namespaced_leaf(
    leaf: &str,
    receiver: Option<&Expr>,
    scope: &Scope,
    span: Span,
) -> Result<Option<String>, RexRapError> {
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
            return Err(RexRapError::semantic(
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
            _ => Err(RexRapError::semantic(
                span,
                format!("namespace '{head}' ({target}) has no function '{leaf}'"),
            )),
        };
    }
    if let Some(exports) = scope.module_aliases.get(head) {
        if keys.len() != 1 {
            return Ok(None);
        }
        return exports.get(leaf).cloned().map(Some).ok_or_else(|| {
            RexRapError::semantic(
                span,
                format!("source module alias '{head}' has no function '{leaf}'"),
            )
        });
    }
    Ok(None)
}

// resolve `std.<module>.<leaf>` to the bare leaf, with a precise error when the module is wrong.
fn resolve_std_leaf(module: &str, leaf: &str, span: Span) -> Result<String, RexRapError> {
    match runinator_compute::resolve_std_path(module, leaf) {
        Ok(_) => Ok(leaf.to_string()),
        Err(Some(actual)) => Err(RexRapError::semantic(
            span,
            format!("no function '{leaf}' in std.{module}; it lives in std.{actual}"),
        )),
        Err(None) => Err(RexRapError::semantic(
            span,
            format!("'std.{module}.{leaf}' is not a builtin function"),
        )),
    }
}

// a bare prefix call must be a user function or an imported intrinsic; a bare prefix call to a
// builtin intrinsic is rejected with guidance to qualify or import it.
fn enforce_prefix_call(name: &mut String, scope: &Scope, span: Span) -> Result<(), RexRapError> {
    if let Some(rewritten) = scope.function_renames.get(name) {
        *name = rewritten.clone();
        return Ok(());
    }
    if scope.user_fns.contains(name) || scope.bare_intrinsics.contains(name) {
        return Ok(());
    }
    if is_known_intrinsic(name) {
        let hint = match intrinsic_module(name) {
            Some(module) => format!("std.{module}.{name}(...) or `import std.{module}`"),
            None => format!("std.<module>.{name}(...)"),
        };
        return Err(RexRapError::semantic(
            span,
            format!("'{name}' is a builtin intrinsic and must be qualified: use {hint}"),
        ));
    }
    // an unknown bare name (likely a user-function typo) is left for sema to report.
    Ok(())
}

// reject a namespace path used as a value (e.g. `std.strings` or an import alias on its own).
fn reject_namespace_value(segs: &[PathSeg], scope: &Scope, span: Span) -> Result<(), RexRapError> {
    let Some(head) = segs.first().and_then(|seg| path_key(seg)) else {
        return Ok(());
    };
    if head == STD_NAMESPACE {
        return Err(RexRapError::semantic(
            span,
            "'std' is a namespace and cannot be used as a value".to_string(),
        ));
    }
    if scope.aliases.contains_key(head) {
        return Err(RexRapError::semantic(
            span,
            format!("'{head}' is an imported namespace and cannot be used as a value"),
        ));
    }
    if scope.module_aliases.contains_key(head) {
        return Err(RexRapError::semantic(
            span,
            format!("'{head}' is a source module and cannot be used as a value"),
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
