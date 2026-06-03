// walks pest pairs into the wdl ast. operator precedence is encoded directly in the
// grammar (cond_or/and/unary, coalesce/concat), so no separate pratt pass is needed.

use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

use crate::ast::*;
use crate::errors::{Span, WdlError};

#[derive(Parser)]
#[grammar = "wdl.pest"]
struct WdlParser;

/// parse wdl source into a Document ast.
pub fn parse_document(src: &str) -> Result<Document, WdlError> {
    let mut pairs =
        WdlParser::parse(Rule::document, src).map_err(|err| WdlError::Parse(err.to_string()))?;
    let document = pairs
        .next()
        .ok_or_else(|| WdlError::Parse("empty input".into()))?;
    let workflow = document
        .into_inner()
        .find(|pair| pair.as_rule() == Rule::workflow)
        .ok_or_else(|| WdlError::Parse("missing workflow".into()))?;
    Ok(Document {
        workflow: parse_workflow(workflow)?,
    })
}

fn span_of(pair: &Pair<Rule>) -> Span {
    let span = pair.as_span();
    Span::new(span.start(), span.end())
}

fn parse_workflow(pair: Pair<Rule>) -> Result<Workflow, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut version = None;
    let mut input = None;
    let mut start = None;
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::version => {
                let digits = inner.as_str().trim_start_matches('v');
                version = Some(parse_i64(digits, span)?);
            }
            Rule::input_block => input = Some(parse_input_block(inner)?),
            Rule::start_decl => start = Some(parse_target(first_inner(inner)?)?),
            Rule::stmt => body.push(parse_stmt(inner)?),
            _ => {}
        }
    }
    Ok(Workflow {
        name,
        version,
        input,
        start,
        body,
        span,
    })
}

// input typing --------------------------------------------------------------

fn parse_input_block(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let fields = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::type_field)
        .map(parse_type_field)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TypeExpr::Struct(fields))
}

fn parse_type_field(pair: Pair<Rule>) -> Result<TypeField, WdlError> {
    let mut name = String::new();
    let mut optional = false;
    let mut ty = TypeExpr::Named("any".into());
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => name = inner.as_str().to_string(),
            Rule::string => name = plain_string(inner)?,
            Rule::optional_mark => optional = true,
            Rule::type_expr => ty = parse_type_expr(inner)?,
            _ => {}
        }
    }
    Ok(TypeField { name, optional, ty })
}

fn parse_type_expr(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    // type_expr -> type_union
    let union = first_inner(pair)?;
    parse_type_union(union)
}

fn parse_type_union(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let mut variants = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::type_postfix)
        .map(parse_type_postfix)
        .collect::<Result<Vec<_>, _>>()?;
    if variants.len() == 1 {
        return Ok(variants.remove(0));
    }
    Ok(TypeExpr::Union(variants))
}

fn parse_type_postfix(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let mut ty = None;
    let mut suffixes = 0;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::type_atom => ty = Some(parse_type_atom(inner)?),
            Rule::array_suffix => suffixes += 1,
            _ => {}
        }
    }
    let mut ty = ty.ok_or_else(|| WdlError::lower("missing type atom"))?;
    for _ in 0..suffixes {
        ty = TypeExpr::Array(Box::new(ty));
    }
    Ok(ty)
}

fn parse_type_atom(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::type_map => {
            let value = first_inner(inner)?;
            Ok(TypeExpr::Map(Box::new(parse_type_expr(value)?)))
        }
        Rule::type_struct => {
            let fields = inner
                .into_inner()
                .filter(|p| p.as_rule() == Rule::type_field)
                .map(parse_type_field)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TypeExpr::Struct(fields))
        }
        Rule::type_named => Ok(TypeExpr::Named(inner.as_str().to_string())),
        other => Err(WdlError::lower(format!("unexpected type atom {other:?}"))),
    }
}

// statements ----------------------------------------------------------------

fn parse_stmt(pair: Pair<Rule>) -> Result<Stmt, WdlError> {
    let span = span_of(&pair);
    let mut annotations = Annotations::default();
    let mut label = None;
    let mut label_type = None;
    let mut kind = None;
    let mut transitions = TransitionClause::default();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::annotation => apply_annotation(&mut annotations, inner)?,
            Rule::binding => {
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::ident => label = Some(part.as_str().to_string()),
                        Rule::type_expr => label_type = Some(parse_type_expr(part)?),
                        _ => {}
                    }
                }
            }
            Rule::stmt_body => kind = Some(parse_stmt_body(inner)?),
            Rule::transitions => transitions = parse_transitions(inner)?,
            _ => {}
        }
    }
    let kind = kind.ok_or_else(|| WdlError::syntax(span, "statement has no body"))?;
    if label.is_some() && !matches!(kind, StmtKind::Action(_) | StmtKind::Subflow(_)) {
        return Err(WdlError::syntax(
            span,
            "let binding is only allowed on action or subflow steps",
        ));
    }
    Ok(Stmt {
        span,
        annotations,
        label,
        label_type,
        kind,
        transitions,
    })
}

fn apply_annotation(annotations: &mut Annotations, pair: Pair<Rule>) -> Result<(), WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::ann_id => {
            let string = first_inner(inner)?;
            annotations.id = Some(plain_string(string)?);
        }
        Rule::ann_skip => annotations.skip = true,
        _ => {}
    }
    Ok(())
}

fn parse_stmt_body(pair: Pair<Rule>) -> Result<StmtKind, WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::action_stmt => Ok(StmtKind::Action(parse_action(inner)?)),
        Rule::subflow_stmt => Ok(StmtKind::Subflow(parse_subflow(inner)?)),
        Rule::wait_stmt => Ok(StmtKind::Wait(parse_wait(inner)?)),
        Rule::emit_stmt => Ok(StmtKind::Emit(parse_emit(inner)?)),
        Rule::approval_stmt => Ok(StmtKind::Approval(parse_approval(inner)?)),
        Rule::config_stmt => Ok(StmtKind::Config(parse_config(inner)?)),
        Rule::fail_stmt => Ok(StmtKind::Fail(parse_fail(inner)?)),
        Rule::if_stmt => Ok(StmtKind::If(parse_if(inner)?)),
        Rule::for_stmt => Ok(StmtKind::For(parse_for(inner)?)),
        Rule::while_stmt => Ok(StmtKind::While(parse_while(inner, false)?)),
        Rule::until_stmt => Ok(StmtKind::While(parse_while(inner, true)?)),
        Rule::match_stmt => Ok(StmtKind::Match(parse_match(inner)?)),
        Rule::parallel_stmt => Ok(StmtKind::Parallel(parse_parallel(inner)?)),
        Rule::try_stmt => Ok(StmtKind::Try(parse_try(inner)?)),
        Rule::race_stmt => Ok(StmtKind::Race(parse_race(inner)?)),
        Rule::map_stmt => Ok(StmtKind::Map(parse_map(inner)?)),
        other => Err(WdlError::lower(format!("unexpected statement {other:?}"))),
    }
}

fn parse_action(pair: Pair<Rule>) -> Result<ActionStmt, WdlError> {
    let span = span_of(&pair);
    let mut idents = Vec::new();
    let mut args = Vec::new();
    let mut modifiers = Modifiers::default();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::action_ident => idents.push(inner.as_str().to_string()),
            Rule::arg_list => args = parse_arg_list(inner)?,
            Rule::modifier => apply_modifier(&mut modifiers, inner)?,
            _ => {}
        }
    }
    if idents.len() != 2 {
        return Err(WdlError::syntax(span, "action requires provider.function"));
    }
    Ok(ActionStmt {
        provider: idents[0].clone(),
        function: idents[1].clone(),
        args,
        modifiers,
    })
}

fn parse_arg_list(pair: Pair<Rule>) -> Result<Vec<(String, Expr)>, WdlError> {
    pair.into_inner()
        .filter(|p| p.as_rule() == Rule::arg)
        .map(|arg| {
            let mut inner = arg.into_inner();
            let name = inner.next().ok_or_else(|| WdlError::lower("arg name"))?;
            let value = inner.next().ok_or_else(|| WdlError::lower("arg value"))?;
            Ok((name.as_str().to_string(), parse_expr(value)?))
        })
        .collect()
}

fn apply_modifier(modifiers: &mut Modifiers, pair: Pair<Rule>) -> Result<(), WdlError> {
    let span = span_of(&pair);
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .ok_or_else(|| WdlError::lower("modifier name"))?
        .as_str()
        .to_string();
    let mut positional = Vec::new();
    let mut named: Vec<(String, Expr)> = Vec::new();
    for arg in inner {
        if arg.as_rule() != Rule::mod_arg_list {
            continue;
        }
        for marg in arg.into_inner().filter(|p| p.as_rule() == Rule::mod_arg) {
            let mut name = None;
            let mut value = None;
            for part in marg.into_inner() {
                match part.as_rule() {
                    Rule::ident => name = Some(part.as_str().to_string()),
                    Rule::expr => value = Some(parse_expr(part)?),
                    _ => {}
                }
            }
            let value = value.ok_or_else(|| WdlError::lower("modifier arg value"))?;
            match name {
                Some(name) => named.push((name, value)),
                None => positional.push(value),
            }
        }
    }
    match name.as_str() {
        "timeout" => {
            modifiers.timeout_seconds = Some(expect_int(positional.first(), "timeout")?);
        }
        "retry" => {
            modifiers.retry = Some(expect_int(positional.first(), "retry")?);
        }
        "tags" => {
            for value in &positional {
                modifiers.tags.push(expect_string(value, "tags")?);
            }
        }
        "mcp" => modifiers.mcp = true,
        "reentry" => {
            let max = named
                .iter()
                .find(|(key, _)| key == "max")
                .map(|(_, value)| value)
                .or_else(|| positional.first());
            let max_visits = expect_int(max, "reentry max")?;
            let on_exhausted = named
                .iter()
                .find(|(key, _)| key == "else" || key == "on_exhausted")
                .map(|(_, value)| value_to_target(value))
                .transpose()?;
            modifiers.reentry = Some(Reentry {
                max_visits,
                on_exhausted,
            });
        }
        other => {
            return Err(WdlError::syntax(
                span,
                format!("unknown modifier '{other}'"),
            ));
        }
    }
    Ok(())
}

fn parse_subflow(pair: Pair<Rule>) -> Result<SubflowStmt, WdlError> {
    let mut workflow_name = String::new();
    let mut detached = false;
    let mut reuse = false;
    let mut run_name = None;
    let mut params = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::subflow_verb => detached = inner.as_str() == "spawn",
            Rule::string => workflow_name = plain_string(inner)?,
            Rule::subflow_opt => {
                let text = inner.as_str();
                if text.starts_with("detached") {
                    detached = true;
                } else if text.starts_with("reuse") {
                    reuse = true;
                } else if let Some(as_pair) =
                    inner.into_inner().find(|p| p.as_rule() == Rule::subflow_as)
                {
                    let expr = first_inner(as_pair)?;
                    run_name = Some(parse_expr(expr)?);
                }
            }
            Rule::subflow_with => {
                let object = first_inner(inner)?;
                params = parse_object_entries(object)?;
            }
            _ => {}
        }
    }
    Ok(SubflowStmt {
        workflow_name,
        detached,
        reuse,
        run_name,
        params,
    })
}

fn parse_wait(pair: Pair<Rule>) -> Result<WaitStmt, WdlError> {
    let mut amount = WaitAmount::Seconds(0);
    let mut until_status = None;
    let mut initial_status = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::wait_amount => {
                let value = first_inner(inner)?;
                amount = match value.as_rule() {
                    Rule::duration => {
                        WaitAmount::Seconds(parse_duration(value.as_str(), span_of(&value))?)
                    }
                    _ => WaitAmount::Expr(parse_expr(value)?),
                };
            }
            Rule::wait_until => until_status = Some(plain_string(first_inner(inner)?)?),
            Rule::wait_initial => initial_status = Some(plain_string(first_inner(inner)?)?),
            _ => {}
        }
    }
    Ok(WaitStmt {
        amount,
        until_status,
        initial_status,
    })
}

fn parse_emit(pair: Pair<Rule>) -> Result<EmitStmt, WdlError> {
    let mut event_type = None;
    let mut data = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => event_type = Some(plain_string(inner)?),
            Rule::object => data = Some(parse_object(inner)?),
            _ => {}
        }
    }
    Ok(EmitStmt { event_type, data })
}

fn parse_approval(pair: Pair<Rule>) -> Result<ApprovalStmt, WdlError> {
    let span = span_of(&pair);
    let mut prompt = Expr::new(
        ExprKind::Str(vec![StrPart::Lit("Approval required".into())]),
        span,
    );
    let mut approval_type = None;
    let mut metadata = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => prompt = parse_expr(inner)?,
            Rule::approval_type => approval_type = Some(plain_string(first_inner(inner)?)?),
            Rule::object => metadata = parse_object_entries(inner)?,
            _ => {}
        }
    }
    Ok(ApprovalStmt {
        approval_type,
        prompt,
        metadata,
    })
}

fn parse_config(pair: Pair<Rule>) -> Result<ConfigStmt, WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::config_name => Ok(ConfigStmt {
            name: Some(parse_expr(first_inner(inner)?)?),
            metadata: None,
        }),
        Rule::config_meta => Ok(ConfigStmt {
            name: None,
            metadata: Some(parse_object(first_inner(inner)?)?),
        }),
        other => Err(WdlError::lower(format!("unexpected config {other:?}"))),
    }
}

fn parse_fail(pair: Pair<Rule>) -> Result<Option<Expr>, WdlError> {
    match pair.into_inner().find(|p| p.as_rule() == Rule::expr) {
        Some(expr) => Ok(Some(parse_expr(expr)?)),
        None => Ok(None),
    }
}

fn parse_if(pair: Pair<Rule>) -> Result<IfStmt, WdlError> {
    let mut arms = Vec::new();
    let mut else_block = None;
    let mut pending_cond = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::cond => pending_cond = Some(parse_cond(inner)?),
            Rule::block => {
                let body = parse_block(inner)?;
                let cond = pending_cond
                    .take()
                    .ok_or_else(|| WdlError::lower("if arm missing condition"))?;
                arms.push((cond, body));
            }
            Rule::else_if => {
                let mut cond = None;
                let mut body = Vec::new();
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::cond => cond = Some(parse_cond(part)?),
                        Rule::block => body = parse_block(part)?,
                        _ => {}
                    }
                }
                arms.push((
                    cond.ok_or_else(|| WdlError::lower("else if missing condition"))?,
                    body,
                ));
            }
            Rule::else_block => {
                else_block = Some(parse_block(first_inner(inner)?)?);
            }
            _ => {}
        }
    }
    Ok(IfStmt { arms, else_block })
}

fn parse_for(pair: Pair<Rule>) -> Result<ForStmt, WdlError> {
    let mut var = String::new();
    let mut items = Expr::new(ExprKind::Null, Span::default());
    let mut limit = None;
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => var = inner.as_str().to_string(),
            Rule::expr => items = parse_expr(inner)?,
            // `limit none` yields no integer child and means uncapped (limit stays None).
            Rule::for_limit => limit = parse_optional_count(inner)?,
            Rule::block => body = parse_block(inner)?,
            _ => {}
        }
    }
    Ok(ForStmt {
        var,
        items,
        limit,
        body,
    })
}

fn parse_while(pair: Pair<Rule>, negate: bool) -> Result<WhileStmt, WdlError> {
    let mut cond = None;
    let mut limit = None;
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::cond => cond = Some(parse_cond(inner)?),
            Rule::for_limit => {
                let int = first_inner(inner)?;
                limit = Some(parse_i64(int.as_str(), span_of(&int))?);
            }
            Rule::block => body = parse_block(inner)?,
            _ => {}
        }
    }
    Ok(WhileStmt {
        cond: cond.ok_or_else(|| WdlError::lower("while loop missing condition"))?,
        negate,
        limit,
        body,
    })
}

fn parse_map(pair: Pair<Rule>) -> Result<MapStmt, WdlError> {
    let mut var = String::new();
    let mut items = Expr::new(ExprKind::Null, Span::default());
    let mut concurrency = None;
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => var = inner.as_str().to_string(),
            Rule::expr => items = parse_expr(inner)?,
            // `concurrency none` yields no integer child and means unbounded (stays None).
            Rule::map_concurrency => concurrency = parse_optional_count(inner)?,
            Rule::block => body = parse_block(inner)?,
            _ => {}
        }
    }
    Ok(MapStmt {
        var,
        items,
        concurrency,
        body,
    })
}

fn parse_match(pair: Pair<Rule>) -> Result<MatchStmt, WdlError> {
    let mut subject = Expr::new(ExprKind::Null, Span::default());
    let mut arms = Vec::new();
    let mut default = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => subject = parse_expr(inner)?,
            Rule::match_arm => arms.push(parse_match_arm(inner)?),
            Rule::match_default => {
                let body = first_inner(inner)?;
                default = Some(parse_arm_body(body)?);
            }
            _ => {}
        }
    }
    Ok(MatchStmt {
        subject,
        arms,
        default,
    })
}

fn parse_match_arm(pair: Pair<Rule>) -> Result<MatchArm, WdlError> {
    let mut equals = None;
    let mut when = None;
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::match_head => {
                let head = first_inner(inner)?;
                match head.as_rule() {
                    Rule::cond => when = Some(parse_cond(head)?),
                    Rule::expr => equals = Some(parse_expr(head)?),
                    _ => {}
                }
            }
            Rule::arm_body => body = parse_arm_body(inner)?,
            _ => {}
        }
    }
    Ok(MatchArm { equals, when, body })
}

fn parse_arm_body(pair: Pair<Rule>) -> Result<Block, WdlError> {
    // arm_body = { block | stmt }
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::block => parse_block(inner),
        Rule::stmt => Ok(vec![parse_stmt(inner)?]),
        other => Err(WdlError::lower(format!("unexpected arm body {other:?}"))),
    }
}

fn parse_parallel(pair: Pair<Rule>) -> Result<ParallelStmt, WdlError> {
    let mut branches = Vec::new();
    let mut join = BranchPolicy::All;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::branch => branches.push(parse_block(first_inner(inner)?)?),
            Rule::join_clause => join = parse_branch_policy(first_inner(inner)?)?,
            _ => {}
        }
    }
    Ok(ParallelStmt { branches, join })
}

fn parse_race(pair: Pair<Rule>) -> Result<RaceStmt, WdlError> {
    let mut branches = Vec::new();
    let mut winner = BranchPolicy::FirstSuccess;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::branch => branches.push(parse_block(first_inner(inner)?)?),
            Rule::winner_clause => winner = parse_branch_policy(first_inner(inner)?)?,
            _ => {}
        }
    }
    Ok(RaceStmt { branches, winner })
}

fn parse_try(pair: Pair<Rule>) -> Result<TryStmt, WdlError> {
    let mut body = Vec::new();
    let mut catch = None;
    let mut finally = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::block => body = parse_block(inner)?,
            Rule::try_catch => catch = Some(parse_block(first_inner(inner)?)?),
            Rule::try_finally => finally = Some(parse_block(first_inner(inner)?)?),
            _ => {}
        }
    }
    Ok(TryStmt {
        body,
        catch,
        finally,
    })
}

fn parse_branch_policy(pair: Pair<Rule>) -> Result<BranchPolicy, WdlError> {
    let span = span_of(&pair);
    match pair.as_str() {
        "all" => Ok(BranchPolicy::All),
        "any" => Ok(BranchPolicy::Any),
        "first_success" => Ok(BranchPolicy::FirstSuccess),
        other => Err(WdlError::syntax(
            span,
            format!("unknown branch policy '{other}'"),
        )),
    }
}

fn parse_block(pair: Pair<Rule>) -> Result<Block, WdlError> {
    pair.into_inner()
        .filter(|p| p.as_rule() == Rule::stmt)
        .map(parse_stmt)
        .collect()
}

// transitions ---------------------------------------------------------------

fn parse_transitions(pair: Pair<Rule>) -> Result<TransitionClause, WdlError> {
    let mut clause = TransitionClause::default();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::single_arrow => {
                let target = parse_target(first_inner(inner)?)?;
                clause.next = Some(target);
            }
            Rule::outcome_arrow => {
                let arrow_span = span_of(&inner);
                let mut outcome = String::new();
                let mut target = None;
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::outcome => outcome = part.as_str().to_string(),
                        Rule::target => target = Some(parse_target(part)?),
                        _ => {}
                    }
                }
                let target = target.ok_or_else(|| WdlError::lower("arrow missing target"))?;
                match outcome.as_str() {
                    "next" => clause.next = Some(target),
                    "ok" => clause.on_success = Some(target),
                    "fail" => clause.on_failure = Some(target),
                    "timeout" => clause.on_timeout = Some(target),
                    "reject" => clause.on_reject = Some(target),
                    other => {
                        return Err(WdlError::syntax(
                            arrow_span,
                            format!("unknown outcome '{other}'"),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(clause)
}

fn parse_target(pair: Pair<Rule>) -> Result<Target, WdlError> {
    match pair.as_str() {
        "done" => Ok(Target::Done),
        "fail" => Ok(Target::Fail),
        other => Ok(Target::Label(other.to_string())),
    }
}

// conditions ----------------------------------------------------------------

fn parse_cond(pair: Pair<Rule>) -> Result<Cond, WdlError> {
    parse_cond_or(first_inner(pair)?)
}

fn parse_cond_or(pair: Pair<Rule>) -> Result<Cond, WdlError> {
    let span = span_of(&pair);
    let mut parts = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::cond_and)
        .map(parse_cond_and)
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() == 1 {
        return Ok(parts.remove(0));
    }
    Ok(Cond::new(CondKind::Any(parts), span))
}

fn parse_cond_and(pair: Pair<Rule>) -> Result<Cond, WdlError> {
    let span = span_of(&pair);
    let mut parts = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::cond_unary)
        .map(parse_cond_unary)
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() == 1 {
        return Ok(parts.remove(0));
    }
    Ok(Cond::new(CondKind::All(parts), span))
}

fn parse_cond_unary(pair: Pair<Rule>) -> Result<Cond, WdlError> {
    let span = span_of(&pair);
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::not_cond => {
            let nested = first_inner(inner)?;
            Ok(Cond::new(
                CondKind::Not(Box::new(parse_cond_unary(nested)?)),
                span,
            ))
        }
        Rule::cond_primary => parse_cond_primary(inner),
        other => Err(WdlError::lower(format!("unexpected cond unary {other:?}"))),
    }
}

fn parse_cond_primary(pair: Pair<Rule>) -> Result<Cond, WdlError> {
    let span = span_of(&pair);
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::paren_cond => parse_cond(first_inner(inner)?),
        Rule::cond_exists => Ok(Cond::new(
            CondKind::Exists(parse_expr(first_inner(inner)?)?),
            span,
        )),
        Rule::cond_cmp => parse_cond_cmp(inner),
        other => Err(WdlError::lower(format!(
            "unexpected cond primary {other:?}"
        ))),
    }
}

fn parse_cond_cmp(pair: Pair<Rule>) -> Result<Cond, WdlError> {
    let span = span_of(&pair);
    let mut left = None;
    let mut op = None;
    let mut right = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => {
                if left.is_none() {
                    left = Some(parse_expr(inner)?);
                } else {
                    right = Some(parse_expr(inner)?);
                }
            }
            Rule::cmp_op => op = Some(parse_cmp_op(inner.as_str(), span_of(&inner))?),
            _ => {}
        }
    }
    Ok(Cond::new(
        CondKind::Cmp {
            left: left.ok_or_else(|| WdlError::lower("comparison missing left"))?,
            op: op.ok_or_else(|| WdlError::lower("comparison missing operator"))?,
            right: right.ok_or_else(|| WdlError::lower("comparison missing right"))?,
        },
        span,
    ))
}

fn parse_cmp_op(text: &str, span: Span) -> Result<CmpOp, WdlError> {
    Ok(match text {
        "==" => CmpOp::Eq,
        "!=" => CmpOp::Ne,
        ">=" => CmpOp::Ge,
        "<=" => CmpOp::Le,
        ">" => CmpOp::Gt,
        "<" => CmpOp::Lt,
        "contains" => CmpOp::Contains,
        "in" => CmpOp::In,
        "starts_with" => CmpOp::StartsWith,
        "ends_with" => CmpOp::EndsWith,
        other => {
            return Err(WdlError::syntax(
                span,
                format!("unknown operator '{other}'"),
            ));
        }
    })
}

// expressions ---------------------------------------------------------------

fn parse_expr(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    // expr -> coalesce_expr
    parse_coalesce(first_inner(pair)?)
}

fn parse_coalesce(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut parts = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::concat_expr)
        .map(parse_concat)
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() == 1 {
        return Ok(parts.remove(0));
    }
    Ok(Expr::new(ExprKind::Coalesce(parts), span))
}

fn parse_concat(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut parts = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::primary)
        .map(parse_primary)
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() == 1 {
        return Ok(parts.remove(0));
    }
    Ok(Expr::new(ExprKind::Concat(parts), span))
}

fn parse_primary(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let inner = first_inner(pair)?;
    let span = span_of(&inner);
    let kind = match inner.as_rule() {
        Rule::paren_expr => return parse_expr(first_inner(inner)?),
        Rule::func_call => return parse_func_call(inner),
        Rule::object => return parse_object(inner),
        Rule::array => return parse_array(inner),
        Rule::duration => ExprKind::Int(parse_duration(inner.as_str(), span)?),
        Rule::number => parse_number(inner.as_str(), span)?,
        Rule::boolean => ExprKind::Bool(inner.as_str() == "true"),
        Rule::null_lit => ExprKind::Null,
        Rule::string => ExprKind::Str(string_parts(inner)?),
        Rule::path => return parse_path(inner),
        other => return Err(WdlError::lower(format!("unexpected primary {other:?}"))),
    };
    Ok(Expr::new(kind, span))
}

fn parse_func_call(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut arg = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::func_name => name = inner.as_str().to_string(),
            Rule::expr => arg = Some(parse_expr(inner)?),
            _ => {}
        }
    }
    let arg =
        Box::new(arg.ok_or_else(|| WdlError::syntax(span, "function call missing argument"))?);
    let kind = match name.as_str() {
        "string" => ExprKind::ToString(arg),
        "json" => ExprKind::ToJson(arg),
        other => {
            return Err(WdlError::syntax(
                span,
                format!("unknown function '{other}'"),
            ));
        }
    };
    Ok(Expr::new(kind, span))
}

fn parse_path(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut segs = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => segs.push(PathSeg::Key(inner.as_str().to_string())),
            Rule::path_seg => {
                let seg = first_inner(inner.clone()).map(|p| p).ok();
                match seg {
                    Some(p) if p.as_rule() == Rule::ident => {
                        segs.push(PathSeg::Key(p.as_str().to_string()))
                    }
                    Some(p) if p.as_rule() == Rule::integer => {
                        let index = parse_i64(p.as_str(), span_of(&p))?;
                        segs.push(PathSeg::Index(index.max(0) as usize));
                    }
                    _ => {
                        // path_seg with a directly captured token
                        let text = inner.as_str();
                        if let Ok(index) = text.parse::<usize>() {
                            segs.push(PathSeg::Index(index));
                        } else {
                            segs.push(PathSeg::Key(text.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if segs.is_empty() {
        return Err(WdlError::syntax(span, "empty path"));
    }
    Ok(Expr::new(ExprKind::Path(segs), span))
}

fn parse_object(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    Ok(Expr::new(
        ExprKind::Object(parse_object_entries(pair)?),
        span,
    ))
}

fn parse_object_entries(pair: Pair<Rule>) -> Result<Vec<(String, Expr)>, WdlError> {
    let mut entries = Vec::new();
    for entry in pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::object_entry)
    {
        let inner = first_inner(entry)?;
        match inner.as_rule() {
            Rule::object_pair => {
                let mut key = String::new();
                let mut value = None;
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::ident => key = part.as_str().to_string(),
                        Rule::string => key = plain_string(part)?,
                        Rule::expr => value = Some(parse_expr(part)?),
                        _ => {}
                    }
                }
                entries.push((key, value.ok_or_else(|| WdlError::lower("object value"))?));
            }
            Rule::object_shorthand => {
                let ident = first_inner(inner)?;
                let span = span_of(&ident);
                let name = ident.as_str().to_string();
                entries.push((
                    name.clone(),
                    Expr::new(ExprKind::Path(vec![PathSeg::Key(name)]), span),
                ));
            }
            _ => {}
        }
    }
    Ok(entries)
}

fn parse_array(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let items = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::expr)
        .map(parse_expr)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Expr::new(ExprKind::Array(items), span))
}

// scalars and strings -------------------------------------------------------

fn parse_number(text: &str, span: Span) -> Result<ExprKind, WdlError> {
    if text.contains('.') {
        text.parse::<f64>()
            .map(ExprKind::Float)
            .map_err(|_| WdlError::syntax(span, format!("invalid number '{text}'")))
    } else {
        text.parse::<i64>()
            .map(ExprKind::Int)
            .map_err(|_| WdlError::syntax(span, format!("invalid integer '{text}'")))
    }
}

fn parse_duration(text: &str, span: Span) -> Result<i64, WdlError> {
    let (digits, unit) = text.split_at(text.len() - 1);
    let amount = digits
        .parse::<i64>()
        .map_err(|_| WdlError::syntax(span, format!("invalid duration '{text}'")))?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        other => {
            return Err(WdlError::syntax(
                span,
                format!("unknown duration unit '{other}'"),
            ));
        }
    };
    Ok(amount * multiplier)
}

fn parse_i64(text: &str, span: Span) -> Result<i64, WdlError> {
    text.parse::<i64>()
        .map_err(|_| WdlError::syntax(span, format!("invalid integer '{text}'")))
}

/// parse a `limit`/`concurrency` clause: `Some(n)` for an integer, `None` for `none`. the
/// `none` literal is not a captured rule, so an absent integer child means uncapped.
fn parse_optional_count(pair: Pair<Rule>) -> Result<Option<i64>, WdlError> {
    match pair.into_inner().find(|p| p.as_rule() == Rule::integer) {
        Some(int) => Ok(Some(parse_i64(int.as_str(), span_of(&int))?)),
        None => Ok(None),
    }
}

fn string_parts(pair: Pair<Rule>) -> Result<Vec<StrPart>, WdlError> {
    let mut parts = Vec::new();
    for inner in pair.into_inner() {
        if inner.as_rule() != Rule::str_part {
            continue;
        }
        let token = first_inner(inner)?;
        match token.as_rule() {
            Rule::str_text => push_lit(&mut parts, token.as_str()),
            Rule::escape => push_lit(&mut parts, &decode_escape(token.as_str())),
            Rule::interpolation => {
                let expr = first_inner(token)?;
                parts.push(StrPart::Expr(parse_expr(expr)?));
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        parts.push(StrPart::Lit(String::new()));
    }
    Ok(parts)
}

fn push_lit(parts: &mut Vec<StrPart>, text: &str) {
    if let Some(StrPart::Lit(last)) = parts.last_mut() {
        last.push_str(text);
    } else {
        parts.push(StrPart::Lit(text.to_string()));
    }
}

fn decode_escape(text: &str) -> String {
    let mut chars = text.chars();
    chars.next(); // backslash
    match chars.next() {
        Some('n') => "\n".to_string(),
        Some('t') => "\t".to_string(),
        Some('r') => "\r".to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn plain_string(pair: Pair<Rule>) -> Result<String, WdlError> {
    let parts = string_parts(pair)?;
    let mut out = String::new();
    for part in parts {
        match part {
            StrPart::Lit(text) => out.push_str(&text),
            StrPart::Expr(_) => {
                return Err(WdlError::lower(
                    "interpolation is not allowed in this position",
                ));
            }
        }
    }
    Ok(out)
}

// helpers used by modifier interpretation -----------------------------------

fn expect_int(value: Option<&Expr>, label: &str) -> Result<i64, WdlError> {
    match value.map(|value| &value.kind) {
        Some(ExprKind::Int(int)) => Ok(*int),
        _ => Err(WdlError::lower(format!("{label} expects an integer"))),
    }
}

fn expect_string(value: &Expr, label: &str) -> Result<String, WdlError> {
    match &value.kind {
        ExprKind::Str(parts) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(text) => Ok(text.clone()),
            StrPart::Expr(_) => Err(WdlError::lower(format!("{label} expects a literal string"))),
        },
        _ => Err(WdlError::lower(format!("{label} expects a string"))),
    }
}

fn value_to_target(value: &Expr) -> Result<Target, WdlError> {
    match &value.kind {
        ExprKind::Path(segs) if segs.len() == 1 => {
            if let PathSeg::Key(name) = &segs[0] {
                return Ok(match name.as_str() {
                    "done" => Target::Done,
                    "fail" => Target::Fail,
                    other => Target::Label(other.to_string()),
                });
            }
            Err(WdlError::lower("invalid target"))
        }
        ExprKind::Str(parts) if parts.len() == 1 => {
            if let StrPart::Lit(name) = &parts[0] {
                return Ok(Target::Label(name.clone()));
            }
            Err(WdlError::lower("invalid target"))
        }
        _ => Err(WdlError::lower("target must be a label")),
    }
}

fn first_inner(pair: Pair<Rule>) -> Result<Pair<Rule>, WdlError> {
    pair.into_inner()
        .next()
        .ok_or_else(|| WdlError::lower("expected child node"))
}
