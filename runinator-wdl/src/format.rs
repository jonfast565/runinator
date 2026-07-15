// renders the parsed wdl ast back to canonical source. comments captured by `attach_comments`
// (see `comments.rs`) are woven back in: leading comments render on their own lines above an
// anchor, a trailing comment suffixes the anchor's last line, and dangling comments render on
// their own lines after the last statement of a block.

use crate::ast::*;
use crate::comments::{Comment, CommentSet};

pub fn format_document(document: &Document) -> String {
    let mut formatter = Formatter {
        out: String::new(),
        indent: 0,
    };
    formatter.document(document);
    formatter.out
}

struct Formatter {
    out: String,
    indent: usize,
}

impl Formatter {
    // render leading comments (each on its own line) at the current indent.
    fn emit_leading(&mut self, comments: &[Comment]) {
        for comment in comments {
            self.emit_comment(comment);
        }
    }

    // render one comment; multi-line block comments keep their interior lines verbatim.
    fn emit_comment(&mut self, comment: &Comment) {
        for line in comment.text.split('\n') {
            self.line(line.trim_end());
        }
    }

    // splice a trailing comment onto the anchor's last emitted line (before its newline).
    fn append_trailing(&mut self, comments: &CommentSet) {
        let Some(trailing) = &comments.trailing else {
            return;
        };
        if self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push(' ');
        self.out.push_str(trailing.text.trim_end());
        self.out.push('\n');
    }

    fn function_def(&mut self, function: &FunctionDef) {
        self.emit_leading(&function.comments.leading);
        if let Some(max_depth) = function.recursive {
            self.line(&format!("@recursive(max_depth: {max_depth})"));
        }
        let signature = format_fn_signature(function);
        match &function.body {
            FnBody::Expr(expr) => {
                self.line(&format!(
                    "fn {}{signature} = {}",
                    function.name,
                    format_expr(expr)
                ));
            }
            // a block body renders like a compute block: each line carries its own indentation and
            // the closing brace hugs the function's base indent.
            FnBody::Block(lines) => {
                let mut out = format!("fn {}{signature} = {{\n", function.name);
                self.indent += 1;
                self.compute_lines(&mut out, lines);
                self.indent -= 1;
                self.push_indent(&mut out);
                out.push('}');
                self.line(&out);
            }
        }
        self.append_trailing(&function.comments);
    }

    fn document(&mut self, document: &Document) {
        // top-level `fn` definitions render first, each on its own line.
        for function in &document.functions {
            self.function_def(function);
        }
        if !document.functions.is_empty() {
            self.out.push('\n');
        }
        for (index, workflow) in document.workflows.iter().enumerate() {
            if index > 0 {
                self.out.push('\n');
            }
            if let Some(namespace) = &workflow.namespace {
                self.line(&format!("namespace {namespace} {{"));
                self.indent += 1;
                self.workflow(workflow);
                self.indent -= 1;
                self.line("}");
            } else {
                self.workflow(workflow);
            }
        }
        // comments after the last top-level item, on their own lines.
        if !document.trailing_comments.is_empty() {
            self.out.push('\n');
            self.emit_leading(&document.trailing_comments);
        }
    }

    fn workflow(&mut self, workflow: &Workflow) {
        self.emit_leading(&workflow.leading_comments);
        let version = workflow
            .version
            .map(|version| format!(" v{version}"))
            .unwrap_or_default();
        let returns = workflow
            .output
            .as_ref()
            .map(|ty| format!(" returns {}", format_type(ty)))
            .unwrap_or_default();
        self.line(&format!(
            "workflow {}{version}{returns} {{",
            quote(&workflow.name)
        ));
        self.indent += 1;
        if let Some(input) = &workflow.input {
            self.params(input);
            if !workflow.triggers.is_empty()
                || !workflow.aliases.is_empty()
                || !workflow.body.is_empty()
                || !workflow.imports.is_empty()
            {
                self.out.push('\n');
            }
        }
        for import in &workflow.imports {
            self.emit_leading(&import.comments.leading);
            match &import.alias {
                Some(alias) => self.line(&format!("import {} as {alias}", import.path)),
                None => self.line(&format!("import {}", import.path)),
            }
            self.append_trailing(&import.comments);
        }
        if (workflow.namespace.is_some() || !workflow.imports.is_empty())
            && (!workflow.triggers.is_empty()
                || !workflow.aliases.is_empty()
                || !workflow.body.is_empty())
        {
            self.out.push('\n');
        }
        // preserve header `trigger cron` declarations.
        for trigger in &workflow.triggers {
            self.emit_leading(&trigger.comments.leading);
            self.trigger_decl(trigger);
            self.append_trailing(&trigger.comments);
        }
        if !workflow.triggers.is_empty()
            && (!workflow.aliases.is_empty() || !workflow.body.is_empty())
        {
            self.out.push('\n');
        }
        // preserve header `watch <cond> -> <target>` cancellation guards.
        for watch in &workflow.watches {
            self.line(&format!(
                "watch {} -> {}",
                format_cond(&watch.cond),
                format_target(&watch.handler)
            ));
        }
        if !workflow.watches.is_empty()
            && (!workflow.type_decls.is_empty()
                || !workflow.aliases.is_empty()
                || !workflow.body.is_empty())
        {
            self.out.push('\n');
        }
        // preserve named `type <Name>` declarations; struct types render each field on its own line.
        for (index, decl) in workflow.type_decls.iter().enumerate() {
            if index > 0 {
                self.out.push('\n');
            }
            self.emit_leading(&decl.comments.leading);
            if let TypeExpr::Struct { fields, additional } = &decl.ty {
                self.type_struct_block(&format!("type {} {{", decl.name), fields, additional);
            } else {
                self.line(&format!("type {} = {}", decl.name, format_type(&decl.ty)));
            }
            self.append_trailing(&decl.comments);
        }
        if !workflow.type_decls.is_empty()
            && (!workflow.aliases.is_empty() || !workflow.body.is_empty())
        {
            self.out.push('\n');
        }
        // preserve header `alias` declarations; they are surface sugar and never reach the graph.
        for alias in &workflow.aliases {
            self.emit_leading(&alias.comments.leading);
            self.alias_decl(alias);
            self.append_trailing(&alias.comments);
        }
        if !workflow.aliases.is_empty() && !workflow.body.is_empty() {
            self.out.push('\n');
        }
        // preserve an explicit `start -> <target>` entry edge when the source declared one.
        if let Some(start) = &workflow.start {
            self.line(&format!("start -> {}", format_target(start)));
        }
        self.block_body(&workflow.body);
        // comments trapped after the last body statement, before the closing brace.
        if !workflow.dangling_comments.is_empty() {
            if !workflow.body.is_empty() {
                self.out.push('\n');
            }
            self.emit_leading(&workflow.dangling_comments);
        }
        self.indent -= 1;
        self.line("}");
    }

    fn params(&mut self, input: &TypeExpr) {
        let TypeExpr::Struct { fields, additional } = input else {
            return;
        };
        self.type_struct_block("params {", fields, additional);
    }

    // render a struct body as a brace block with one field per line, sharing the shape used by
    // `params` and named `type` declarations.
    fn type_struct_block(
        &mut self,
        header: &str,
        fields: &[TypeField],
        additional: &Option<Box<TypeExpr>>,
    ) {
        self.line(header);
        self.indent += 1;
        for field in fields {
            self.type_field(field, false);
        }
        if let Some(additional) = additional {
            self.line(&format!("...: {}", format_type(additional)));
        }
        self.indent -= 1;
        self.line("}");
    }

    fn trigger_decl(&mut self, trigger: &TriggerDecl) {
        let mut text = match &trigger.kind {
            TriggerDeclKind::Cron { schedule, .. } => {
                format!("trigger cron {}", format_expr(schedule))
            }
            TriggerDeclKind::Chained { event, target } => {
                format!(
                    "trigger {} workflow {}",
                    event.keyword(),
                    format_expr(target)
                )
            }
        };
        if let Some(params) = &trigger.params {
            text.push_str(&format!(" with {}", format_expr(params)));
        }
        if !trigger.enabled {
            text.push_str(" disabled");
        }
        if let TriggerDeclKind::Cron {
            blackout_start: Some(start),
            blackout_end: Some(end),
            ..
        } = &trigger.kind
        {
            text.push_str(&format!(
                " blackout {} to {}",
                format_expr(start),
                format_expr(end)
            ));
        }
        self.line(&text);
    }

    fn alias_decl(&mut self, alias: &Alias) {
        let body = format_object_entries_multiline(&alias.entries, self.indent);
        self.line(&format!("alias {} = {body}", alias.name));
    }

    fn type_field(&mut self, field: &TypeField, comma: bool) {
        self.emit_leading(&field.comments.leading);
        let optional = if field.optional { "?" } else { "" };
        let name = format_key(&field.name);
        match &field.ty {
            TypeExpr::Struct { fields, additional } if !fields.is_empty() => {
                self.line(&format!("{name}{optional}: {{"));
                self.indent += 1;
                let has_additional = additional.is_some();
                for (index, nested) in fields.iter().enumerate() {
                    self.type_field(nested, index + 1 < fields.len() || has_additional);
                }
                if let Some(additional) = additional {
                    self.line(&format!("...: {}", format_type(additional)));
                }
                self.indent -= 1;
                let suffix = if comma { "," } else { "" };
                self.line(&format!("}}{suffix}"));
            }
            ty => {
                let suffix = if comma { "," } else { "" };
                // a default implies optionality, so it replaces the `?` marker rather than adding to it.
                if let Some(default) = &field.default {
                    self.line(&format!(
                        "{name}: {} = {}{suffix}",
                        format_type(ty),
                        format_expr(default)
                    ));
                } else {
                    self.line(&format!("{name}{optional}: {}{suffix}", format_type(ty)));
                }
            }
        }
        self.append_trailing(&field.comments);
        // comments trapped after the last field of a struct render before its closing brace.
        self.emit_leading(&field.comments.dangling);
    }

    fn block_body(&mut self, body: &Block) {
        // render each statement in isolation, then rejoin with a blank line between any pair where
        // either statement spans multiple lines, so multi-line statements never look crushed.
        let mut pieces: Vec<String> = Vec::with_capacity(body.len());
        for stmt in body {
            let previous = std::mem::take(&mut self.out);
            self.stmt(stmt);
            pieces.push(std::mem::replace(&mut self.out, previous));
        }
        for (index, piece) in pieces.iter().enumerate() {
            if index > 0 && (is_multiline_piece(&pieces[index - 1]) || is_multiline_piece(piece)) {
                self.out.push('\n');
            }
            self.out.push_str(piece);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        self.emit_leading(&stmt.comments.leading);
        if let Some(id) = &stmt.annotations.id {
            self.line(&format!("@id({})", quote(id)));
        }
        if stmt.annotations.skip {
            self.line("@skip");
        }
        if stmt.annotations.locked {
            self.line("@lock");
        }
        if let Some(timeout) = stmt.annotations.timeout_seconds {
            self.line(&format!("@timeout({timeout}s)"));
        }

        let mut text = String::new();
        if let Some(label) = &stmt.label {
            text.push_str("node ");
            text.push_str(label);
            if let Some(label_type) = &stmt.label_type {
                text.push_str(": ");
                text.push_str(&format_type(label_type));
            }
            text.push_str(" <- ");
        }

        text.push_str(&self.stmt_kind(&stmt.kind));
        if let Some(compensation) = &stmt.compensation {
            text.push_str(&format!(" compensate {}", self.action(compensation)));
        }
        if stmt.transitions.is_empty() {
            self.line(&text);
        } else {
            self.stmt_with_transitions(&text, &stmt.transitions);
        }
        self.append_trailing(&stmt.comments);
        // dangling comments trapped at the end of this statement's block render after it.
        self.emit_leading(&stmt.comments.dangling);
    }

    fn stmt_with_transitions(&mut self, text: &str, transitions: &TransitionClause) {
        // gather every outgoing edge into one `edges { … }` section under the statement, matching
        // the decompiler's canonical shape so formatting never reflows the arrows differently.
        let mut edges: Vec<(&str, &Target)> = Vec::new();
        for (outcome, target) in [
            ("next", &transitions.next),
            ("ok", &transitions.on_success),
            ("fail", &transitions.on_failure),
            ("timeout", &transitions.on_timeout),
            ("reject", &transitions.on_reject),
        ] {
            if let Some(target) = target {
                edges.push((outcome, target));
            }
        }

        self.line(text);
        if edges.is_empty() && transitions.branches.is_empty() {
            return;
        }
        self.line("edges {");
        self.indent += 1;
        for (outcome, target) in edges {
            self.line(&format!("{outcome} -> {}", format_target(target)));
        }
        // user-defined predicate edges, in declaration order, mirroring the decompiler's rendering.
        for branch in &transitions.branches {
            let cond = format_cond(&branch.when);
            let target = format_target(&branch.target);
            match branch.priority {
                Some(priority) => {
                    self.line(&format!("when {cond} priority {priority} -> {target}"))
                }
                None => self.line(&format!("when {cond} -> {target}")),
            }
        }
        self.indent -= 1;
        self.line("}");
    }

    fn stmt_kind(&mut self, kind: &StmtKind) -> String {
        match kind {
            StmtKind::Action(action) => self.action(action),
            StmtKind::Compute(compute) => self.compute(compute),
            StmtKind::Subflow(subflow) => self.subflow(subflow),
            StmtKind::Wait(wait) => self.wait(wait),
            StmtKind::Output(output) => self.output(output),
            StmtKind::Yield(value) => format!("yield {}", format_expr(value)),
            StmtKind::Input(input) => self.input_stmt(input),
            StmtKind::Approval(approval) => self.approval(approval),
            StmtKind::Gate(gate) => self.gate(gate),
            StmtKind::Signal(signal) => self.signal(signal),
            StmtKind::Assert(assert) => self.assert(assert),
            StmtKind::Transform(transform) => self.transform(transform),
            StmtKind::Audit(audit) => self.audit(audit),
            StmtKind::Checkpoint(checkpoint) => {
                format!("checkpoint {}", quote(&checkpoint.name))
            }
            StmtKind::Mutex(mutex) => self.mutex(mutex),
            StmtKind::Throttle(throttle) => self.throttle(throttle),
            StmtKind::Await(await_stmt) => self.await_node(await_stmt),
            StmtKind::Debounce(debounce) => self.debounce(debounce),
            StmtKind::Collect(collect) => self.collect(collect),
            StmtKind::Barrier(barrier) => self.barrier(barrier),
            StmtKind::CircuitBreaker(cb) => self.circuit_breaker(cb),
            StmtKind::EventSource(es) => self.event_source(es),
            StmtKind::Config(config) => self.config(config),
            StmtKind::Fail(expr) => match expr {
                Some(expr) => format!("fail {}", format_expr(expr)),
                None => "fail".to_string(),
            },
            StmtKind::If(if_stmt) => self.if_stmt(if_stmt),
            StmtKind::For(for_stmt) => self.for_stmt(for_stmt),
            StmtKind::While(while_stmt) => self.while_stmt(while_stmt),
            StmtKind::Match(match_stmt) => self.match_stmt(match_stmt),
            StmtKind::Parallel(parallel) => self.parallel(parallel),
            StmtKind::Try(try_stmt) => self.try_stmt(try_stmt),
            StmtKind::Race(race) => self.race(race),
            StmtKind::Map(map) => self.map(map),
        }
    }

    fn compute(&mut self, compute: &ComputeStmt) -> String {
        let mut out = match &compute.foreign {
            Some(foreign) => self.foreign_compute(foreign),
            None => {
                let mut out = String::from("compute {\n");
                self.indent += 1;
                self.compute_lines(&mut out, &compute.body);
                self.indent -= 1;
                self.push_indent(&mut out);
                out.push('}');
                out
            }
        };
        // render trailing modifiers (e.g. `.timeout(30s)`) like an action call.
        let mut modifiers = Vec::new();
        if let Some(seconds) = compute.modifiers.timeout_seconds {
            modifiers.push(format!(".timeout({seconds}s)"));
        }
        if let Some(retry) = &compute.modifiers.retry {
            modifiers.push(format_retry(retry));
        }
        self.append_modifiers(&mut out, &modifiers, true);
        out
    }

    fn foreign_compute(&self, foreign: &ForeignCompute) -> String {
        let mut out = format!("compute {}", quote(&foreign.language));
        out.push_str(" ```\n");
        out.push_str(&foreign.source);
        if !foreign.source.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```");
        out
    }

    fn compute_lines(&mut self, out: &mut String, body: &[ComputeLine]) {
        for line in body {
            match line {
                ComputeLine::Let { name, ty, value } => {
                    let ty = ty
                        .as_ref()
                        .map(|ty| format!(": {}", format_type(ty)))
                        .unwrap_or_default();
                    self.push_indent(out);
                    out.push_str(&format!("let {name}{ty} = {}\n", format_expr(value)));
                }
                ComputeLine::Return(value) => {
                    self.push_indent(out);
                    out.push_str(&format!("return {}\n", format_expr(value)));
                }
                ComputeLine::Goto(target) => {
                    self.push_indent(out);
                    out.push_str(&format!("goto {}\n", format_target(target)));
                }
                ComputeLine::Expr(value) => {
                    self.push_indent(out);
                    out.push_str(&format!("{}\n", format_expr(value)));
                }
                ComputeLine::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    self.push_indent(out);
                    out.push_str(&format!("if {} {{\n", format_cond(cond)));
                    self.indent += 1;
                    self.compute_lines(out, then_branch);
                    self.indent -= 1;
                    if else_branch.is_empty() {
                        self.push_indent(out);
                        out.push_str("}\n");
                    } else {
                        self.push_indent(out);
                        out.push_str("} else {\n");
                        self.indent += 1;
                        self.compute_lines(out, else_branch);
                        self.indent -= 1;
                        self.push_indent(out);
                        out.push_str("}\n");
                    }
                }
            }
        }
    }

    fn action(&self, action: &ActionStmt) -> String {
        let multiline = !action.args.is_empty();
        let args = if multiline {
            self.action_args_multiline(&action.args)
        } else {
            "()".to_string()
        };
        let mut text = format!("{}.{}{args}", action.provider, action.function);
        self.action_modifiers(action, &mut text, multiline);
        text
    }

    // arguments render in source order; a `...alias` spread entry prints as `...alias`.
    fn action_args_multiline(&self, args: &[(String, Expr)]) -> String {
        if args.is_empty() {
            return "()".to_string();
        }
        let arg_indent = self.indent + 1;
        let mut out = "(\n".to_string();
        for (index, (name, value)) in args.iter().enumerate() {
            out.push_str(&indent(arg_indent));
            match &value.kind {
                ExprKind::Spread(alias) => out.push_str(&format!("...{alias}")),
                _ => {
                    out.push_str(name);
                    out.push_str(": ");
                    out.push_str(&format_expr_multiline(value, arg_indent));
                }
            }
            if index + 1 < args.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&indent(self.indent));
        out.push(')');
        out
    }

    fn action_modifiers(&self, action: &ActionStmt, text: &mut String, multiline: bool) {
        let mut modifiers = Vec::new();
        if let Some(seconds) = action.modifiers.timeout_seconds {
            modifiers.push(format!(".timeout({seconds}s)"));
        }
        if let Some(retry) = &action.modifiers.retry {
            modifiers.push(format_retry(retry));
        }
        if !action.modifiers.tags.is_empty() {
            let tags = action
                .modifiers
                .tags
                .iter()
                .map(|tag| quote(tag))
                .collect::<Vec<_>>()
                .join(", ");
            modifiers.push(format!(".tags({tags})"));
        }
        if action.modifiers.mcp {
            modifiers.push(".mcp()".to_string());
        }
        if let Some(runner) = &action.modifiers.runner {
            modifiers.push(format!(".runner({})", quote(runner)));
        }
        if let Some(reentry) = &action.modifiers.reentry {
            let mut modifier = format!(".reentry(max: {}", reentry.max_visits);
            if let Some(target) = &reentry.on_exhausted {
                modifier.push_str(&format!(", else: {}", format_target(target)));
            }
            modifier.push(')');
            modifiers.push(modifier);
        }
        self.append_modifiers(text, &modifiers, multiline);
    }

    // attach the fluent modifier chain. the first call hugs the closing paren/brace; any further
    // calls align their leading dot one column past it. an inline call keeps the chain on one line.
    fn append_modifiers(&self, text: &mut String, modifiers: &[String], multiline: bool) {
        let Some((first, rest)) = modifiers.split_first() else {
            return;
        };
        text.push_str(first);
        if !multiline {
            for modifier in rest {
                text.push_str(modifier);
            }
            return;
        }
        let pad = format!("{} ", indent(self.indent));
        for modifier in rest {
            text.push('\n');
            text.push_str(&pad);
            text.push_str(modifier);
        }
    }

    fn subflow(&self, subflow: &SubflowStmt) -> String {
        let mut args = vec![quote(&subflow.workflow_name)];
        if !subflow.params.is_empty() {
            args.push(format!(
                "params: {}",
                format_object_entries_multiline(&subflow.params, self.indent)
            ));
        }
        if subflow.reuse {
            args.push("reuse: true".to_string());
        }
        if subflow.detached {
            args.push("detached: true".to_string());
        }
        if let Some(run_name) = &subflow.run_name {
            args.push(format!("name: {}", format_expr(run_name)));
        }
        format!("subflow({})", args.join(", "))
    }

    fn wait(&self, wait: &WaitStmt) -> String {
        let amount = match &wait.amount {
            WaitAmount::Seconds(seconds) => format!("{seconds}s"),
            WaitAmount::Expr(expr) => format_expr(expr),
        };
        let mut text = format!("wait {amount}");
        if let Some(status) = &wait.until_status {
            text.push_str(&format!(" until {}", quote(status)));
        }
        if let Some(status) = &wait.initial_status {
            text.push_str(&format!(" initial {}", quote(status)));
        }
        text
    }

    fn output(&mut self, output: &OutputStmt) -> String {
        if !output.items.is_empty() {
            // block form when artifact items are declared.
            let mut out = String::from("output {\n");
            self.indent += 1;
            let has_event = output.event_type.is_some() || output.data.is_some();
            if has_event {
                self.push_indent(&mut out);
                out.push_str("emit");
                if let Some(event_type) = &output.event_type {
                    out.push_str(&format!(" {}", quote(event_type)));
                }
                if let Some(data) = &output.data {
                    let rendered = format_expr_multiline(data, self.indent);
                    if output.event_type.is_some() || matches!(data.kind, ExprKind::Object(_)) {
                        out.push_str(&format!(" {rendered}"));
                    } else {
                        out.push_str(&format!(" ({rendered})"));
                    }
                }
                out.push('\n');
            }
            for (name, source) in &output.items {
                self.push_indent(&mut out);
                out.push_str(&format!("{name} = {}\n", format_expr(source)));
            }
            self.indent -= 1;
            self.push_indent(&mut out);
            out.push('}');
            return out;
        }
        // shorthand emit form for event-only nodes.
        let mut text = "emit".to_string();
        if let Some(event_type) = &output.event_type {
            text.push_str(&format!(" {}", quote(event_type)));
        }
        if let Some(data) = &output.data {
            let rendered = format_expr_multiline(data, self.indent);
            // object payloads keep their brace form; an event-less scalar is parenthesized so it
            // is not re-parsed as the event type. mirrors the decompiler.
            if output.event_type.is_some() || matches!(data.kind, ExprKind::Object(_)) {
                text.push_str(&format!(" {rendered}"));
            } else {
                text.push_str(&format!(" ({rendered})"));
            }
        }
        text
    }

    fn input_stmt(&self, input: &InputStmt) -> String {
        let mut text = "input".to_string();
        if let Some(prompt) = &input.prompt {
            text.push(' ');
            text.push_str(&format_expr(prompt));
        }
        text
    }

    fn approval(&self, approval: &ApprovalStmt) -> String {
        let mut text = format!("approve {}", format_expr(&approval.prompt));
        if let Some(approval_type) = &approval.approval_type {
            text.push_str(&format!(" type {}", quote(approval_type)));
        }
        if !approval.metadata.is_empty() {
            text.push_str(&format!(
                " {}",
                format_object_entries_multiline(&approval.metadata, self.indent)
            ));
        }
        text
    }

    fn gate(&self, gate: &GateStmt) -> String {
        let mut text = format!("gate {}", gate.kind);
        if let Some(when) = &gate.when {
            text.push_str(&format!(" when {}", format_cond(when)));
        }
        if let Some(poll) = gate.poll_interval {
            text.push_str(&format!(" every {poll}s"));
        }
        if let Some(timeout) = gate.timeout {
            text.push_str(&format!(" timeout {timeout}s"));
        }
        if !gate.metadata.is_empty() {
            text.push_str(&format!(
                " {}",
                format_object_entries_multiline(&gate.metadata, self.indent)
            ));
        }
        text
    }

    fn signal(&self, signal: &SignalStmt) -> String {
        let mut text = format!("signal {}", quote(&signal.name));
        if let Some(key) = &signal.correlation_key {
            text.push_str(&format!(" key {}", format_expr(key)));
        }
        if !signal.metadata.is_empty() {
            text.push_str(&format!(
                " {}",
                format_object_entries_multiline(&signal.metadata, self.indent)
            ));
        }
        text
    }

    fn config(&self, config: &ConfigStmt) -> String {
        if let Some(name) = &config.name {
            return format!("set name = {}", format_expr(name));
        }
        if let Some(metadata) = &config.metadata {
            return format!("set meta {}", format_expr_multiline(metadata, self.indent));
        }
        "set meta {}".to_string()
    }

    fn assert(&mut self, assert: &AssertStmt) -> String {
        if assert.assertions.is_empty() {
            return "assert {}".to_string();
        }
        let mut out = String::from("assert {\n");
        self.indent += 1;
        for (name, cond) in &assert.assertions {
            self.push_indent(&mut out);
            out.push_str(&format!("{}: {}\n", quote(name), format_cond(cond)));
        }
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
    }

    fn transform(&mut self, transform: &TransformStmt) -> String {
        if transform.bindings.is_empty() {
            return "transform {}".to_string();
        }
        let mut out = String::from("transform {\n");
        self.indent += 1;
        for (name, value) in &transform.bindings {
            self.push_indent(&mut out);
            out.push_str(&format!("{name} = {}\n", format_expr(value)));
        }
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
    }

    fn audit(&self, audit: &AuditStmt) -> String {
        let mut text = format!("audit action {}", format_expr(&audit.action));
        if let Some(actor) = &audit.actor {
            text.push_str(&format!(" actor {}", format_expr(actor)));
        }
        if let Some(target) = &audit.target {
            text.push_str(&format!(" target {}", format_expr(target)));
        }
        if let Some(reason) = &audit.reason {
            text.push_str(&format!(" reason {}", format_expr(reason)));
        }
        text
    }

    fn mutex(&mut self, mutex: &MutexStmt) -> String {
        // a bare release leaf carries only the lock name.
        if mutex.release {
            return format!("mutex release {}", quote(&mutex.name));
        }
        let mut header = format!("mutex {}", quote(&mutex.name));
        if let Some(poll) = mutex.poll_interval {
            header.push_str(&format!(" every {poll}s"));
        }
        if let Some(timeout) = mutex.timeout {
            header.push_str(&format!(" timeout {timeout}s"));
        }
        if let Some(hold) = mutex.hold {
            header.push_str(&format!(" hold {hold}s"));
        }
        if mutex.body.is_empty() {
            return header;
        }
        header.push_str(" {");
        self.render_block(&header, &mutex.body, "}")
            .trim_end_matches('\n')
            .to_string()
    }

    fn throttle(&self, throttle: &ThrottleStmt) -> String {
        let mut text = format!(
            "throttle {} rate {} per {}s",
            quote(&throttle.name),
            throttle.max_per_window,
            throttle.window_seconds
        );
        if let Some(poll) = throttle.poll_interval {
            text.push_str(&format!(" every {poll}s"));
        }
        if let Some(timeout) = throttle.timeout {
            text.push_str(&format!(" timeout {timeout}s"));
        }
        text
    }

    fn await_node(&self, await_stmt: &AwaitStmt) -> String {
        let mut text = format!("await {}", format_expr(&await_stmt.run_ids));
        if let Some(mode) = &await_stmt.mode {
            text.push_str(&format!(" mode {}", quote(mode)));
        }
        if let Some(poll) = await_stmt.poll_interval {
            text.push_str(&format!(" every {poll}s"));
        }
        if let Some(timeout) = await_stmt.timeout {
            text.push_str(&format!(" timeout {timeout}s"));
        }
        text
    }

    fn debounce(&self, debounce: &DebounceStmt) -> String {
        let mut text = format!(
            "debounce {} delay {}s",
            quote(&debounce.name),
            debounce.delay_seconds
        );
        if let Some(key) = &debounce.key {
            text.push_str(&format!(" key {}", format_expr(key)));
        }
        text
    }

    fn collect(&self, collect: &CollectStmt) -> String {
        let mut text = format!("collect {} max {}", quote(&collect.name), collect.max);
        if let Some(timeout) = collect.timeout {
            text.push_str(&format!(" timeout {timeout}s"));
        }
        text
    }

    fn barrier(&self, barrier: &BarrierStmt) -> String {
        let mut text = format!("barrier {} count {}", quote(&barrier.name), barrier.count);
        if let Some(poll) = barrier.poll_interval {
            text.push_str(&format!(" every {poll}s"));
        }
        if let Some(timeout) = barrier.timeout {
            text.push_str(&format!(" timeout {timeout}s"));
        }
        text
    }

    fn circuit_breaker(&self, cb: &CircuitBreakerStmt) -> String {
        format!(
            "circuit_breaker {} threshold {} window {}s cooldown {}s",
            quote(&cb.name),
            cb.threshold,
            cb.window_seconds,
            cb.cooldown_seconds
        )
    }

    fn event_source(&self, es: &EventSourceStmt) -> String {
        let mut text = format!("event_source type {}", quote(&es.event_type));
        if let Some(filter) = &es.filter {
            text.push_str(&format!(" filter {}", format_cond(filter)));
        }
        if let Some(max) = es.max {
            text.push_str(&format!(" max {max}"));
        }
        if let Some(timeout) = es.timeout {
            text.push_str(&format!(" timeout {timeout}s"));
        }
        text
    }

    fn if_stmt(&mut self, if_stmt: &IfStmt) -> String {
        let mut out = String::new();
        for (index, (cond, body)) in if_stmt.arms.iter().enumerate() {
            let head = if index == 0 {
                format!("if {} {{", format_cond(cond))
            } else {
                format!(" else if {} {{", format_cond(cond))
            };
            if index == 0 {
                out.push_str(&self.render_block(&head, body, "}"));
            } else {
                out.pop();
                out.push_str(&self.render_block(&head, body, "}"));
            }
        }
        if let Some(body) = &if_stmt.else_block {
            out.pop();
            out.push_str(&self.render_block(" else {", body, "}"));
        }
        out.trim_end_matches('\n').to_string()
    }

    fn for_stmt(&mut self, for_stmt: &ForStmt) -> String {
        let mut header = format!("for {} in {}", for_stmt.var, format_expr(&for_stmt.items));
        if let Some(limit) = &for_stmt.limit {
            header.push_str(&format!(" limit {}", format_expr(limit)));
        }
        header.push_str(" {");
        self.render_block(&header, &for_stmt.body, "}")
            .trim_end_matches('\n')
            .to_string()
    }

    fn while_stmt(&mut self, while_stmt: &WhileStmt) -> String {
        let keyword = if while_stmt.negate { "until" } else { "while" };
        let mut header = format!("{keyword} {}", format_cond(&while_stmt.cond));
        if let Some(limit) = while_stmt.limit {
            header.push_str(&format!(" limit {limit}"));
        }
        header.push_str(" {");
        self.render_block(&header, &while_stmt.body, "}")
            .trim_end_matches('\n')
            .to_string()
    }

    fn match_stmt(&mut self, match_stmt: &MatchStmt) -> String {
        match match_stmt.mode {
            SwitchMode::Cases => self.match_cases(match_stmt),
            SwitchMode::Toggle => self.toggle_stmt(match_stmt),
            SwitchMode::Percentage => self.split_stmt(match_stmt),
        }
    }

    fn match_cases(&mut self, match_stmt: &MatchStmt) -> String {
        let mut out = String::new();
        out.push_str(&format!("match {} {{\n", format_expr(&match_stmt.subject)));
        self.indent += 1;
        for arm in &match_stmt.arms {
            self.push_indent(&mut out);
            let head = match (&arm.equals, &arm.when) {
                (Some(expr), _) => format_expr(expr),
                (_, Some(cond)) => format!("when {}", format_cond(cond)),
                _ => "when exists null".to_string(),
            };
            out.push_str(&format!("{head} -> {{\n"));
            self.indent += 1;
            self.push_block_into(&mut out, &arm.body);
            self.indent -= 1;
            self.push_indent(&mut out);
            out.push_str("}\n");
        }
        self.push_default_arm(&mut out, match_stmt.default.as_ref());
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
    }

    fn toggle_stmt(&mut self, match_stmt: &MatchStmt) -> String {
        let mut out = String::new();
        out.push_str(&format!("toggle {} {{\n", format_expr(&match_stmt.subject)));
        self.indent += 1;
        // render `on` before `off` regardless of source order.
        for want in [true, false] {
            let Some(arm) = match_stmt.arms.iter().find(|arm| arm.toggle == Some(want)) else {
                continue;
            };
            self.push_indent(&mut out);
            out.push_str(if want { "on -> {\n" } else { "off -> {\n" });
            self.indent += 1;
            self.push_block_into(&mut out, &arm.body);
            self.indent -= 1;
            self.push_indent(&mut out);
            out.push_str("}\n");
        }
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
    }

    fn split_stmt(&mut self, match_stmt: &MatchStmt) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "split on {} {{\n",
            format_expr(&match_stmt.subject)
        ));
        self.indent += 1;
        for arm in &match_stmt.arms {
            self.push_indent(&mut out);
            out.push_str(&format!("{}% -> {{\n", arm.weight.unwrap_or_default()));
            self.indent += 1;
            self.push_block_into(&mut out, &arm.body);
            self.indent -= 1;
            self.push_indent(&mut out);
            out.push_str("}\n");
        }
        self.push_default_arm(&mut out, match_stmt.default.as_ref());
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
    }

    fn push_default_arm(&mut self, out: &mut String, default: Option<&Block>) {
        let Some(body) = default else {
            return;
        };
        self.push_indent(out);
        out.push_str("else -> {\n");
        self.indent += 1;
        self.push_block_into(out, body);
        self.indent -= 1;
        self.push_indent(out);
        out.push_str("}\n");
    }

    fn parallel(&mut self, parallel: &ParallelStmt) -> String {
        let mut out = "parallel {\n".to_string();
        self.indent += 1;
        for branch in &parallel.branches {
            self.push_indent(&mut out);
            out.push_str("branch {\n");
            self.indent += 1;
            self.push_block_into(&mut out, branch);
            self.indent -= 1;
            self.push_indent(&mut out);
            out.push_str("}\n");
        }
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push_str(&format!("}} join {}", format_branch_policy(parallel.join)));
        out
    }

    fn try_stmt(&mut self, try_stmt: &TryStmt) -> String {
        let mut out = self.render_block("try {", &try_stmt.body, "}");
        if let Some(catch) = &try_stmt.catch {
            out.pop();
            out.push_str(&self.render_block(" catch {", catch, "}"));
        }
        if let Some(finally) = &try_stmt.finally {
            out.pop();
            out.push_str(&self.render_block(" finally {", finally, "}"));
        }
        out.trim_end_matches('\n').to_string()
    }

    fn race(&mut self, race: &RaceStmt) -> String {
        let mut out = format!("race winner {} {{\n", format_branch_policy(race.winner));
        self.indent += 1;
        for branch in &race.branches {
            self.push_indent(&mut out);
            out.push_str("branch {\n");
            self.indent += 1;
            self.push_block_into(&mut out, branch);
            self.indent -= 1;
            self.push_indent(&mut out);
            out.push_str("}\n");
        }
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
    }

    fn map(&mut self, map: &MapStmt) -> String {
        let mut header = format!("map {} in {}", map.var, format_expr(&map.items));
        if let Some(concurrency) = map.concurrency {
            header.push_str(&format!(" concurrency {concurrency}"));
        }
        header.push_str(" {");
        self.render_block(&header, &map.body, "}")
            .trim_end_matches('\n')
            .to_string()
    }

    fn render_block(&mut self, header: &str, body: &Block, closing: &str) -> String {
        let mut out = String::new();
        out.push_str(header);
        out.push('\n');
        self.indent += 1;
        self.push_block_into(&mut out, body);
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push_str(closing);
        out.push('\n');
        out
    }

    fn push_block_into(&mut self, out: &mut String, body: &Block) {
        let previous = std::mem::take(&mut self.out);
        self.block_body(body);
        out.push_str(&self.out);
        self.out = previous;
    }

    fn line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn push_indent(&self, out: &mut String) {
        for _ in 0..self.indent {
            out.push_str("    ");
        }
    }
}

/// render a function's surface signature — the parenthesized parameter list plus an optional
/// `-> ret` — without the leading `fn <name>` or the body. used by the formatter and persisted as a
/// decompile hint so the typed `fn` header can be reconstructed from the runtime form.
pub(crate) fn format_fn_signature(function: &FunctionDef) -> String {
    let params = function
        .params
        .iter()
        .map(|param| {
            let optional = if param.optional { "?" } else { "" };
            let default = param
                .default
                .as_ref()
                .map(|expr| format!(" = {}", format_expr(expr)))
                .unwrap_or_default();
            format!(
                "{}{optional}: {}{default}",
                param.name,
                format_type(&param.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = function
        .ret
        .as_ref()
        .map(|ty| format!(" -> {}", format_type(ty)))
        .unwrap_or_default();
    format!("({params}){ret}")
}

fn format_expr(expr: &Expr) -> String {
    format_expr_at(expr, ExprPrec::Lowest)
}

fn format_expr_multiline(expr: &Expr, indent_level: usize) -> String {
    format_expr_at_multiline(expr, ExprPrec::Lowest, indent_level)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ExprPrec {
    Lowest,
    // a ternary binds loosest of the operators (just above a lambda).
    Ternary,
    // a relational comparison binds looser than every value operator.
    Compare,
    // compute arithmetic tiers, looser than coalesce/concat (which sit inside `cprimary`).
    Sum,
    Product,
    Unary,
    Coalesce,
    Concat,
    Primary,
}

// render a left-associative binary arithmetic chain, rendering each operand at the operator level.
fn format_binary(parts: &[Expr], sep: &str, prec: ExprPrec) -> String {
    parts
        .iter()
        .map(|part| format_expr_at(part, prec))
        .collect::<Vec<_>>()
        .join(sep)
}

/// whether an apply callee needs its own parentheses. a key-terminated path (`obj.f` or a bare
/// `f`) would re-parse as a method/prefix call when followed by `(`; an index-terminated path
/// (`fns[0]`), a call result, or a nested apply re-parses unambiguously as an application.
fn apply_callee_needs_parens(callee: &Expr) -> bool {
    matches!(
        &callee.kind,
        ExprKind::Path(segs) if !matches!(segs.last(), Some(PathSeg::Index(_)))
    )
}

fn format_expr_at(expr: &Expr, parent: ExprPrec) -> String {
    let (prec, text) = match &expr.kind {
        ExprKind::Null => (ExprPrec::Primary, "null".to_string()),
        ExprKind::Bool(value) => (ExprPrec::Primary, value.to_string()),
        ExprKind::Int(value) => (ExprPrec::Primary, value.to_string()),
        ExprKind::Float(value) => (ExprPrec::Primary, value.to_string()),
        ExprKind::Str(parts) => (ExprPrec::Primary, format_string_parts(parts)),
        ExprKind::FileInclude { path } => (ExprPrec::Primary, format!("file({})", quote(path))),
        ExprKind::DirInclude {
            path,
            recursive,
            max_depth,
        } => {
            let mut text = format!("dir({}", quote(path));
            if *recursive || max_depth.is_some() {
                text.push_str(&format!(", {recursive}"));
            }
            if let Some(depth) = max_depth {
                text.push_str(&format!(", {depth}"));
            }
            text.push(')');
            (ExprPrec::Primary, text)
        }
        ExprKind::InlineCode { language, content } => {
            (ExprPrec::Primary, format_inline_code(language, content))
        }
        ExprKind::Path(segs) => (ExprPrec::Primary, format_path(segs)),
        ExprKind::Array(items) => {
            let items = items.iter().map(format_expr).collect::<Vec<_>>().join(", ");
            (ExprPrec::Primary, format!("[{items}]"))
        }
        ExprKind::Object(entries) => (ExprPrec::Primary, format_object_entries(entries)),
        ExprKind::Concat(parts) => {
            let parts = parts
                .iter()
                .map(|part| format_expr_at(part, ExprPrec::Concat))
                .collect::<Vec<_>>()
                .join(" ++ ");
            (ExprPrec::Concat, parts)
        }
        ExprKind::Coalesce(parts) => {
            let parts = parts
                .iter()
                .map(|part| format_expr_at(part, ExprPrec::Coalesce))
                .collect::<Vec<_>>()
                .join(" ?? ");
            (ExprPrec::Coalesce, parts)
        }
        ExprKind::ToString(inner) => (ExprPrec::Primary, format!("string({})", format_expr(inner))),
        ExprKind::ToJson(inner) => (ExprPrec::Primary, format!("json({})", format_expr(inner))),
        ExprKind::Spread(name) => (ExprPrec::Primary, format!("...{name}")),
        ExprKind::Add(parts) => (ExprPrec::Sum, format_binary(parts, " + ", ExprPrec::Sum)),
        ExprKind::Sub(parts) => (ExprPrec::Sum, format_binary(parts, " - ", ExprPrec::Sum)),
        ExprKind::Mul(parts) => (
            ExprPrec::Product,
            format_binary(parts, " * ", ExprPrec::Product),
        ),
        ExprKind::Div(parts) => (
            ExprPrec::Product,
            format_binary(parts, " / ", ExprPrec::Product),
        ),
        ExprKind::Mod(parts) => (
            ExprPrec::Product,
            format_binary(parts, " % ", ExprPrec::Product),
        ),
        ExprKind::Neg(inner) => (
            ExprPrec::Unary,
            format!("-{}", format_expr_at(inner, ExprPrec::Unary)),
        ),
        ExprKind::Compare { op, left, right } => (
            ExprPrec::Compare,
            format!(
                "{} {} {}",
                format_expr_at(left, ExprPrec::Compare),
                op.token(),
                format_expr_at(right, ExprPrec::Compare),
            ),
        ),
        ExprKind::Ternary { cond, then, els } => (
            ExprPrec::Ternary,
            format!(
                "{} ? {} : {}",
                format_expr_at(cond, ExprPrec::Compare),
                format_expr(then),
                format_expr(els),
            ),
        ),
        ExprKind::Call {
            name,
            args,
            named,
            method,
        } => {
            // re-sugar `at(base, key)` into `base.key` / `base[index]` access syntax.
            let rendered = if name == "at"
                && args.len() == 2
                && named.is_empty()
                && let Some(access) = format_access(&args[0], &args[1])
            {
                access
            } else if *method && !args.is_empty() {
                // a method-origin call renders fluent as `receiver.name(rest)`. for a namespaced
                // call (`std.strings.upper(x)`) the receiver is the namespace path, so this is also
                // how a qualified intrinsic call round-trips through the formatter.
                let receiver = format_expr_at(&args[0], ExprPrec::Primary);
                let mut rest = args[1..].iter().map(format_expr).collect::<Vec<_>>();
                rest.extend(
                    named
                        .iter()
                        .map(|(key, value)| format!("{key}: {}", format_expr(value))),
                );
                format!("{receiver}.{name}({})", rest.join(", "))
            } else {
                // a prefix call: user functions and zero-arg calls render bare.
                let mut rendered = args.iter().map(format_expr).collect::<Vec<_>>();
                rendered.extend(
                    named
                        .iter()
                        .map(|(key, value)| format!("{key}: {}", format_expr(value))),
                );
                format!("{name}({})", rendered.join(", "))
            };
            (ExprPrec::Primary, rendered)
        }
        ExprKind::Lambda { params, body } => {
            // a single param renders bare (`x => …`); zero or many parenthesize.
            let head = if params.len() == 1 {
                params[0].clone()
            } else {
                format!("({})", params.join(", "))
            };
            (ExprPrec::Lowest, format!("{head} => {}", format_expr(body)))
        }
        // `expr as Type` binds just below a ternary; the inner renders at compare level.
        ExprKind::Cast { expr, ty } => (
            ExprPrec::Ternary,
            format!(
                "{} as {}",
                format_expr_at(expr, ExprPrec::Compare),
                format_type(ty)
            ),
        ),
        // `callee(args)` applies a value. `format_expr_at` already parenthesizes a sub-primary callee
        // (lambda/ternary/operator); a key-terminated path callee (`obj.f`, `f`) additionally needs
        // parentheses or the trailing `(` re-parses as a `obj.f(args)` method / prefix call.
        ExprKind::Apply { callee, args } => {
            let args = args.iter().map(format_expr).collect::<Vec<_>>().join(", ");
            let callee_text = format_expr_at(callee, ExprPrec::Primary);
            let callee_text = if apply_callee_needs_parens(callee) {
                format!("({callee_text})")
            } else {
                callee_text
            };
            (ExprPrec::Primary, format!("{callee_text}({args})"))
        }
    };

    if prec < parent {
        format!("({text})")
    } else {
        text
    }
}

fn format_expr_at_multiline(expr: &Expr, parent: ExprPrec, indent_level: usize) -> String {
    match &expr.kind {
        ExprKind::Object(entries) => {
            let text = format_object_entries_multiline(entries, indent_level);
            if ExprPrec::Primary < parent {
                format!("({text})")
            } else {
                text
            }
        }
        ExprKind::Array(items) if items.iter().any(contains_object) => {
            let items = items
                .iter()
                .map(|item| format_expr_multiline(item, indent_level))
                .collect::<Vec<_>>()
                .join(", ");
            let text = format!("[{items}]");
            if ExprPrec::Primary < parent {
                format!("({text})")
            } else {
                text
            }
        }
        ExprKind::Concat(parts) => {
            let text = parts
                .iter()
                .map(|part| format_expr_at_multiline(part, ExprPrec::Concat, indent_level))
                .collect::<Vec<_>>()
                .join(" ++ ");
            if ExprPrec::Concat < parent {
                format!("({text})")
            } else {
                text
            }
        }
        ExprKind::Coalesce(parts) => {
            let text = parts
                .iter()
                .map(|part| format_expr_at_multiline(part, ExprPrec::Coalesce, indent_level))
                .collect::<Vec<_>>()
                .join(" ?? ");
            if ExprPrec::Coalesce < parent {
                format!("({text})")
            } else {
                text
            }
        }
        ExprKind::ToString(inner) => {
            format!("string({})", format_expr_multiline(inner, indent_level))
        }
        ExprKind::ToJson(inner) => format!("json({})", format_expr_multiline(inner, indent_level)),
        _ => format_expr_at(expr, parent),
    }
}

fn format_cond(cond: &Cond) -> String {
    format_cond_at(cond, CondPrec::Lowest)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CondPrec {
    Lowest,
    Or,
    And,
    Unary,
    Primary,
}

fn format_cond_at(cond: &Cond, parent: CondPrec) -> String {
    let (prec, text) = match &cond.kind {
        CondKind::Any(parts) => {
            let parts = parts
                .iter()
                .map(|part| format_cond_at(part, CondPrec::Or))
                .collect::<Vec<_>>()
                .join(" || ");
            (CondPrec::Or, parts)
        }
        CondKind::All(parts) => {
            let parts = parts
                .iter()
                .map(|part| format_cond_at(part, CondPrec::And))
                .collect::<Vec<_>>()
                .join(" && ");
            (CondPrec::And, parts)
        }
        CondKind::Not(inner) => (
            CondPrec::Unary,
            if matches!(&inner.as_ref().kind, CondKind::Expr(_)) {
                format!("!({})", format_cond_at(inner, CondPrec::Lowest))
            } else {
                format!("!{}", format_cond_at(inner, CondPrec::Unary))
            },
        ),
        CondKind::Expr(expr) => (CondPrec::Primary, format_expr(expr)),
        CondKind::Cmp { left, op, right } => (
            CondPrec::Primary,
            format!(
                "{} {} {}",
                format_expr(left),
                format_cmp_op(*op),
                format_expr(right)
            ),
        ),
        CondKind::Exists(expr) => (CondPrec::Primary, format!("exists {}", format_expr(expr))),
    };

    if prec < parent {
        format!("({text})")
    } else {
        text
    }
}

fn format_object_entries(entries: &[(String, Expr)]) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }
    let parts = entries
        .iter()
        .map(|(key, value)| match &value.kind {
            ExprKind::Spread(name) => format!("...{name}"),
            _ => format!("{}: {}", format_key(key), format_expr(value)),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {parts} }}")
}

fn format_object_entries_multiline(entries: &[(String, Expr)], indent_level: usize) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }
    let mut out = "{\n".to_string();
    for (index, (key, value)) in entries.iter().enumerate() {
        out.push_str(&indent(indent_level + 1));
        match &value.kind {
            ExprKind::Spread(name) => out.push_str(&format!("...{name}")),
            _ => {
                out.push_str(&format_key(key));
                out.push_str(": ");
                out.push_str(&format_expr_multiline(value, indent_level + 1));
            }
        }
        if index + 1 < entries.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&indent(indent_level));
    out.push('}');
    out
}

fn format_string_parts(parts: &[StrPart]) -> String {
    let mut out = String::new();
    out.push('"');
    for part in parts {
        match part {
            StrPart::Lit(text) => out.push_str(&escape_string_lit(text)),
            StrPart::Expr(expr) => out.push_str(&format!("${{{}}}", format_expr(expr))),
        }
    }
    out.push('"');
    out
}

fn format_inline_code(language: &str, content: &str) -> String {
    if content.contains("```") {
        return quote(content);
    }
    let mut out = format!("inline({}, ```\n", quote(language));
    out.push_str(content);
    if !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```)");
    out
}

// re-sugar an `at(base, key)` call into access syntax, or None to keep the call form. a static key
// on a path base is left as a call: `path.key` would fold back into the path, changing the ast.
fn format_access(base: &Expr, key: &Expr) -> Option<String> {
    let base_is_path = matches!(&base.kind, ExprKind::Path(_));
    let static_key = foldable_access_key(key);
    if base_is_path && static_key.is_some() {
        return None;
    }
    let base_text = format_expr_at(base, ExprPrec::Primary);
    if let ExprKind::Int(index) = &key.kind {
        return Some(format!("{base_text}[{index}]"));
    }
    if let ExprKind::Str(parts) = &key.kind
        && let Some(text) = literal_string(parts)
    {
        if is_ident(&text) {
            return Some(format!("{base_text}.{text}"));
        }
        return Some(format!("{base_text}[{}]", quote(&text)));
    }
    // a dynamic key (a ref/call/arithmetic expression) renders as a bracketed expression.
    Some(format!("{base_text}[{}]", format_expr(key)))
}

// the static path segment a key would fold into a path base: a non-negative int or a literal string.
fn foldable_access_key(key: &Expr) -> Option<()> {
    match &key.kind {
        ExprKind::Int(index) if *index >= 0 => Some(()),
        ExprKind::Str(parts) if literal_string(parts).is_some() => Some(()),
        _ => None,
    }
}

// the text of a string with no interpolation, or None when any part is an embedded expression.
fn literal_string(parts: &[StrPart]) -> Option<String> {
    let mut text = String::new();
    for part in parts {
        match part {
            StrPart::Lit(lit) => text.push_str(lit),
            StrPart::Expr(_) => return None,
        }
    }
    Some(text)
}

fn format_path(segs: &[PathSeg]) -> String {
    let mut out = String::new();
    for (index, seg) in segs.iter().enumerate() {
        if index > 0 {
            out.push('.');
        }
        match seg {
            PathSeg::Key(key) => out.push_str(key),
            PathSeg::Index(index) => out.push_str(&index.to_string()),
        }
    }
    out
}

/// render a `TypeExpr` as wdl type syntax, preserving any declared type names. shared with lowering
/// so use-site type annotations can be recorded in a name-preserving form for decompile.
pub(crate) fn format_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(name) => name.clone(),
        TypeExpr::Enum(values) => format!(
            "enum[{}]",
            values
                .iter()
                .map(format_type_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeExpr::Range { base, min, max } => format!(
            "{} range {}..{}",
            format_type(base),
            min.as_ref().map(format_type_value).unwrap_or_default(),
            max.as_ref().map(format_type_value).unwrap_or_default()
        ),
        TypeExpr::Array(inner) => format!("{}[]", format_type(inner)),
        TypeExpr::Map(inner) => format!("map<{}>", format_type(inner)),
        TypeExpr::Struct { fields, additional } => {
            if fields.is_empty() && additional.is_none() {
                return "{}".to_string();
            }
            let mut parts = fields.iter().map(format_type_field).collect::<Vec<_>>();
            if let Some(additional) = additional {
                parts.push(format!("...: {}", format_type(additional)));
            }
            format!("{{ {} }}", parts.join(", "))
        }
        TypeExpr::Union(variants) => variants
            .iter()
            .map(format_type)
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::Function { params, ret } => {
            let params = params
                .iter()
                .map(format_type)
                .collect::<Vec<_>>()
                .join(", ");
            format!("function<({params}) -> {}>", format_type(ret))
        }
    }
}

fn format_type_value(value: &runinator_models::value::Value) -> String {
    match value {
        runinator_models::value::Value::String(text) => quote(text),
        other => other.to_string(),
    }
}

pub(crate) fn format_type_field(field: &TypeField) -> String {
    let optional = if field.optional { "?" } else { "" };
    format!(
        "{}{}: {}",
        format_key(&field.name),
        optional,
        format_type(&field.ty)
    )
}

fn format_target(target: &Target) -> String {
    match target {
        Target::Label(label) => label.clone(),
        Target::Done => "done".to_string(),
        Target::Fail => "fail".to_string(),
    }
}

fn format_branch_policy(policy: BranchPolicy) -> &'static str {
    match policy {
        BranchPolicy::All => "all",
        BranchPolicy::Any => "any",
        BranchPolicy::FirstSuccess => "first_success",
    }
}

fn format_cmp_op(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
        CmpOp::Gt => ">",
        CmpOp::Ge => ">=",
        CmpOp::Lt => "<",
        CmpOp::Le => "<=",
        CmpOp::Contains => "contains",
        CmpOp::In => "in",
        CmpOp::StartsWith => "starts_with",
        CmpOp::EndsWith => "ends_with",
    }
}

fn format_key(key: &str) -> String {
    if is_ident(key) {
        key.to_string()
    } else {
        quote(key)
    }
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    out.push_str(&escape_string_lit(text));
    out.push('"');
    out
}

fn escape_string_lit(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

fn is_ident(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn contains_object(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Object(entries) => !entries.is_empty(),
        ExprKind::Array(items) | ExprKind::Concat(items) | ExprKind::Coalesce(items) => {
            items.iter().any(contains_object)
        }
        ExprKind::ToString(inner) | ExprKind::ToJson(inner) => contains_object(inner),
        ExprKind::Str(parts) => parts.iter().any(|part| match part {
            StrPart::Expr(expr) => contains_object(expr),
            StrPart::Lit(_) => false,
        }),
        _ => false,
    }
}

fn indent(level: usize) -> String {
    "    ".repeat(level)
}

// render a `.retry(...)` call, emitting only the non-default named args so a plain `.retry(3)`
// round-trips unchanged.
fn format_retry(retry: &RetryConfig) -> String {
    let mut args = vec![retry.max_attempts.to_string()];
    if let Some(base) = retry.backoff_base_seconds {
        args.push(format!("backoff: {base}s"));
    }
    if let Some(max) = retry.backoff_max_seconds {
        args.push(format!("max: {max}s"));
    }
    if retry.jitter {
        args.push("jitter: true".to_string());
    }
    if let Some(on) = &retry.retry_on {
        args.push(format!("on: {on}"));
    }
    format!(".retry({})", args.join(", "))
}

// a rendered statement spans multiple lines if its text, sans the trailing newline, contains one.
fn is_multiline_piece(piece: &str) -> bool {
    piece.trim_end_matches('\n').contains('\n')
}
