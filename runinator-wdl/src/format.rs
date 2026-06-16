// renders the parsed wdl ast back to canonical source. comments are intentionally not
// preserved because they are skipped by the grammar before ast construction.

use crate::ast::*;

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
    fn function_def(&mut self, function: &FunctionDef) {
        if let Some(max_depth) = function.recursive {
            self.line(&format!("@recursive(max_depth: {max_depth})"));
        }
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
        self.line(&format!(
            "fn {}({params}){ret} = {}",
            function.name,
            format_expr(&function.body)
        ));
    }

    fn document(&mut self, document: &Document) {
        // top-level `fn` definitions render first, each on its own line.
        for function in &document.functions {
            self.function_def(function);
        }
        if !document.functions.is_empty() {
            self.out.push('\n');
        }
        let workflow = &document.workflow;
        let version = workflow
            .version
            .map(|version| format!(" v{version}"))
            .unwrap_or_default();
        self.line(&format!("workflow {}{version} {{", quote(&workflow.name)));
        self.indent += 1;
        if let Some(input) = &workflow.input {
            self.params(input);
            if !workflow.triggers.is_empty()
                || !workflow.aliases.is_empty()
                || !workflow.body.is_empty()
                || workflow.namespace.is_some()
                || !workflow.imports.is_empty()
            {
                self.out.push('\n');
            }
        }
        // preserve the `namespace` header and `import` declarations (surface sugar that qualifies
        // identity and opens namespaces into local scope).
        if let Some(namespace) = &workflow.namespace {
            self.line(&format!("namespace {namespace}"));
        }
        for import in &workflow.imports {
            match &import.alias {
                Some(alias) => self.line(&format!("import {} as {alias}", import.path)),
                None => self.line(&format!("import {}", import.path)),
            }
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
            self.trigger_decl(trigger);
        }
        if !workflow.triggers.is_empty()
            && (!workflow.aliases.is_empty() || !workflow.body.is_empty())
        {
            self.out.push('\n');
        }
        // preserve named `type <Name>` declarations; struct types render each field on its own line.
        for (index, decl) in workflow.type_decls.iter().enumerate() {
            if index > 0 {
                self.out.push('\n');
            }
            if let TypeExpr::Struct { fields, additional } = &decl.ty {
                self.type_struct_block(&format!("type {} {{", decl.name), fields, additional);
            } else {
                self.line(&format!("type {} = {}", decl.name, format_type(&decl.ty)));
            }
        }
        if !workflow.type_decls.is_empty()
            && (!workflow.aliases.is_empty() || !workflow.body.is_empty())
        {
            self.out.push('\n');
        }
        // preserve header `alias` declarations; they are surface sugar and never reach the graph.
        for alias in &workflow.aliases {
            self.alias_decl(alias);
        }
        if !workflow.aliases.is_empty() && !workflow.body.is_empty() {
            self.out.push('\n');
        }
        // preserve an explicit `start -> <target>` entry edge when the source declared one.
        if let Some(start) = &workflow.start {
            self.line(&format!("start -> {}", format_target(start)));
        }
        self.block_body(&workflow.body);
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
        let schedule = format_expr(&trigger.schedule);
        let mut text = format!("trigger cron {schedule}");
        if let Some(params) = &trigger.params {
            text.push_str(&format!(" with {}", format_expr(params)));
        }
        if !trigger.enabled {
            text.push_str(" disabled");
        }
        if let (Some(start), Some(end)) = (&trigger.blackout_start, &trigger.blackout_end) {
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
        // action, subflow, and compute are the node-leaves: they carry the `node` keyword (with an
        // optional `label[: type] =` binding). every other statement stays bare.
        let is_node_leaf = matches!(
            stmt.kind,
            StmtKind::Action(_) | StmtKind::Subflow(_) | StmtKind::Compute(_)
        );
        if is_node_leaf {
            text.push_str("node ");
            if let Some(label) = &stmt.label {
                text.push_str(label);
                if let Some(label_type) = &stmt.label_type {
                    text.push_str(": ");
                    text.push_str(&format_type(label_type));
                }
                text.push_str(" = ");
            }
        }

        text.push_str(&self.stmt_kind(&stmt.kind));
        if stmt.transitions.is_empty() {
            self.line(&text);
            return;
        }
        self.stmt_with_transitions(&text, &stmt.transitions);
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
            StmtKind::Deliverable(deliverable) => self.deliverable(deliverable),
            StmtKind::Input(input) => self.input_stmt(input),
            StmtKind::Approval(approval) => self.approval(approval),
            StmtKind::Gate(gate) => self.gate(gate),
            StmtKind::Signal(signal) => self.signal(signal),
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
        let mut out = String::from("compute {\n");
        self.indent += 1;
        self.compute_lines(&mut out, &compute.body);
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        // render trailing modifiers (e.g. `.timeout(30s)`) like an action call.
        let mut modifiers = Vec::new();
        if let Some(seconds) = compute.modifiers.timeout_seconds {
            modifiers.push(format!(".timeout({seconds}s)"));
        }
        if let Some(retry) = compute.modifiers.retry {
            modifiers.push(format!(".retry({retry})"));
        }
        self.append_modifiers(&mut out, &modifiers, true);
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
        if let Some(retry) = action.modifiers.retry {
            modifiers.push(format!(".retry({retry})"));
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
        let verb = if subflow.detached { "spawn" } else { "call" };
        let mut text = format!("{verb} {}", quote(&subflow.workflow_name));
        if subflow.reuse {
            text.push_str(" reuse");
        }
        if let Some(run_name) = &subflow.run_name {
            text.push_str(&format!(" as {}", format_expr(run_name)));
        }
        if !subflow.params.is_empty() {
            text.push_str(" with ");
            text.push_str(&format_object_entries_multiline(
                &subflow.params,
                self.indent,
            ));
        }
        text
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

    fn output(&self, output: &OutputStmt) -> String {
        let mut text = "output".to_string();
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

    fn deliverable(&mut self, deliverable: &DeliverableStmt) -> String {
        let mut out = String::from("deliverable {\n");
        self.indent += 1;
        for (name, source) in &deliverable.items {
            self.push_indent(&mut out);
            out.push_str(&format!("{name} = {}\n", format_expr(source)));
        }
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
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
        if let Some(limit) = for_stmt.limit {
            header.push_str(&format!(" limit {limit}"));
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
        if let Some(body) = &match_stmt.default {
            self.push_indent(&mut out);
            out.push_str("else -> {\n");
            self.indent += 1;
            self.push_block_into(&mut out, body);
            self.indent -= 1;
            self.push_indent(&mut out);
            out.push_str("}\n");
        }
        self.indent -= 1;
        self.push_indent(&mut out);
        out.push('}');
        out
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

// a rendered statement spans multiple lines if its text, sans the trailing newline, contains one.
fn is_multiline_piece(piece: &str) -> bool {
    piece.trim_end_matches('\n').contains('\n')
}
