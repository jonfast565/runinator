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
    fn document(&mut self, document: &Document) {
        let workflow = &document.workflow;
        let version = workflow
            .version
            .map(|version| format!(" v{version}"))
            .unwrap_or_default();
        self.line(&format!("workflow {}{version} {{", quote(&workflow.name)));
        self.indent += 1;
        if let Some(input) = &workflow.input {
            self.input(input);
            if !workflow.aliases.is_empty() || !workflow.body.is_empty() {
                self.out.push('\n');
            }
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

    fn input(&mut self, input: &TypeExpr) {
        let TypeExpr::Struct(fields) = input else {
            return;
        };
        self.line("input {");
        self.indent += 1;
        for field in fields {
            self.type_field(field, false);
        }
        self.indent -= 1;
        self.line("}");
    }

    fn alias_decl(&mut self, alias: &Alias) {
        let body = format_object_entries_multiline(&alias.entries, self.indent);
        self.line(&format!("alias {} = {body}", alias.name));
    }

    fn type_field(&mut self, field: &TypeField, comma: bool) {
        let optional = if field.optional { "?" } else { "" };
        let name = format_key(&field.name);
        match &field.ty {
            TypeExpr::Struct(fields) if !fields.is_empty() => {
                self.line(&format!("{name}{optional}: {{"));
                self.indent += 1;
                for (index, nested) in fields.iter().enumerate() {
                    self.type_field(nested, index + 1 < fields.len());
                }
                self.indent -= 1;
                let suffix = if comma { "," } else { "" };
                self.line(&format!("}}{suffix}"));
            }
            ty => {
                let suffix = if comma { "," } else { "" };
                self.line(&format!("{name}{optional}: {}{suffix}", format_type(ty)));
            }
        }
    }

    fn block_body(&mut self, body: &Block) {
        for stmt in body {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        if let Some(id) = &stmt.annotations.id {
            self.line(&format!("@id({})", quote(id)));
        }
        if stmt.annotations.skip {
            self.line("@skip");
        }

        let mut text = String::new();
        if let Some(label) = &stmt.label {
            text.push_str("let ");
            text.push_str(label);
            if let Some(label_type) = &stmt.label_type {
                text.push_str(": ");
                text.push_str(&format_type(label_type));
            }
            text.push_str(" = ");
        }

        text.push_str(&self.stmt_kind(&stmt.kind));
        if stmt.transitions.is_empty() {
            self.line(&text);
            return;
        }
        self.stmt_with_transitions(&text, &stmt.transitions);
    }

    fn stmt_with_transitions(&mut self, text: &str, transitions: &TransitionClause) {
        if let Some(target) = &transitions.next {
            self.line(&format!("{text} -> {}", format_target(target)));
            return;
        }

        self.line(text);
        self.indent += 1;
        for (outcome, target) in [
            ("ok", &transitions.on_success),
            ("fail", &transitions.on_failure),
            ("timeout", &transitions.on_timeout),
            ("reject", &transitions.on_reject),
        ] {
            if let Some(target) = target {
                self.line(&format!("{outcome} -> {}", format_target(target)));
            }
        }
        self.indent -= 1;
    }

    fn stmt_kind(&mut self, kind: &StmtKind) -> String {
        match kind {
            StmtKind::Action(action) => self.action(action),
            StmtKind::Subflow(subflow) => self.subflow(subflow),
            StmtKind::Wait(wait) => self.wait(wait),
            StmtKind::Emit(emit) => self.emit(emit),
            StmtKind::Approval(approval) => self.approval(approval),
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

    fn action(&self, action: &ActionStmt) -> String {
        let args = if action.args.is_empty() {
            "()".to_string()
        } else {
            self.action_args_multiline(&action.args)
        };
        let mut text = format!("{}.{}{args}", action.provider, action.function);
        self.action_modifiers(action, &mut text);
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

    fn action_modifiers(&self, action: &ActionStmt, text: &mut String) {
        if let Some(seconds) = action.modifiers.timeout_seconds {
            self.push_modifier(text, &format!(".timeout({seconds}s)"));
        }
        if let Some(retry) = action.modifiers.retry {
            self.push_modifier(text, &format!(".retry({retry})"));
        }
        if !action.modifiers.tags.is_empty() {
            let tags = action
                .modifiers
                .tags
                .iter()
                .map(|tag| quote(tag))
                .collect::<Vec<_>>()
                .join(", ");
            self.push_modifier(text, &format!(".tags({tags})"));
        }
        if action.modifiers.mcp {
            self.push_modifier(text, ".mcp()");
        }
        if let Some(reentry) = &action.modifiers.reentry {
            let mut modifier = format!(".reentry(max: {}", reentry.max_visits);
            if let Some(target) = &reentry.on_exhausted {
                modifier.push_str(&format!(", else: {}", format_target(target)));
            }
            modifier.push(')');
            self.push_modifier(text, &modifier);
        }
    }

    fn push_modifier(&self, text: &mut String, modifier: &str) {
        text.push('\n');
        text.push_str(&indent(self.indent + 1));
        text.push_str(modifier);
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

    fn emit(&self, emit: &EmitStmt) -> String {
        let mut text = "emit".to_string();
        if let Some(event_type) = &emit.event_type {
            text.push_str(&format!(" {}", quote(event_type)));
        }
        if let Some(data) = &emit.data {
            let rendered = format_expr_multiline(data, self.indent);
            // object payloads keep their brace form; an event-less scalar is parenthesized so it
            // is not re-parsed as the event type. mirrors the decompiler.
            if emit.event_type.is_some() || matches!(data.kind, ExprKind::Object(_)) {
                text.push_str(&format!(" {rendered}"));
            } else {
                text.push_str(&format!(" ({rendered})"));
            }
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
    Coalesce,
    Concat,
    Primary,
}

fn format_expr_at(expr: &Expr, parent: ExprPrec) -> String {
    let (prec, text) = match &expr.kind {
        ExprKind::Null => (ExprPrec::Primary, "null".to_string()),
        ExprKind::Bool(value) => (ExprPrec::Primary, value.to_string()),
        ExprKind::Int(value) => (ExprPrec::Primary, value.to_string()),
        ExprKind::Float(value) => (ExprPrec::Primary, value.to_string()),
        ExprKind::Str(parts) => (ExprPrec::Primary, format_string_parts(parts)),
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
            format!("!{}", format_cond_at(inner, CondPrec::Unary)),
        ),
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

fn format_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(name) => name.clone(),
        TypeExpr::Array(inner) => format!("{}[]", format_type(inner)),
        TypeExpr::Map(inner) => format!("map<{}>", format_type(inner)),
        TypeExpr::Struct(fields) => {
            if fields.is_empty() {
                return "{}".to_string();
            }
            let fields = fields
                .iter()
                .map(format_type_field)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {fields} }}")
        }
        TypeExpr::Union(variants) => variants
            .iter()
            .map(format_type)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn format_type_field(field: &TypeField) -> String {
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
