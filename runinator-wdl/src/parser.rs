// walks pest pairs into the wdl ast. operator precedence is encoded directly in the
// grammar (cond_or/and/unary, coalesce/concat), so no separate pratt pass is needed.

use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

use runinator_models::semver::SemVer;
use runinator_models::value::Value;

use crate::ast::*;
use crate::comments::{CommentSet, attach_comments};
use crate::errors::{Span, WdlError};

#[derive(Parser)]
#[grammar = "wdl.pest"]
struct WdlParser;

/// borrow the workflow currently being assembled, or report the body declaration that appeared
/// before any `workflow` header.
fn require_active<'a>(
    active: &'a mut Option<Workflow>,
    inner: &Pair<Rule>,
) -> Result<&'a mut Workflow, WdlError> {
    active.as_mut().ok_or_else(|| {
        WdlError::syntax(
            span_of(inner),
            "workflow declaration must come before workflow body declarations",
        )
    })
}

/// parse wdl source into a Document ast.
pub fn parse_document(src: &str) -> Result<Document, WdlError> {
    let mut pairs =
        WdlParser::parse(Rule::document, src).map_err(|err| WdlError::Parse(err.to_string()))?;
    let document = pairs
        .next()
        .ok_or_else(|| WdlError::Parse("empty input".into()))?;
    let mut functions = Vec::new();
    let mut workflows = Vec::new();
    let mut active: Option<Workflow> = None;
    let mut current_namespace: Option<String> = None;
    for item in document.into_inner() {
        let inner = match item.as_rule() {
            Rule::document_item => first_inner(item)?,
            Rule::EOI => continue,
            _ => item,
        };
        match inner.as_rule() {
            Rule::func_def => functions.push(parse_func_def(inner)?),
            Rule::namespace_decl => {
                current_namespace = Some(first_inner(inner)?.as_str().to_string());
                if let Some(workflow) = active.as_mut() {
                    workflow.namespace = current_namespace.clone();
                }
            }
            Rule::namespace_block => {
                if let Some(workflow) = active.take() {
                    workflows.push(workflow);
                }
                workflows.extend(parse_namespace_block(inner)?);
            }
            Rule::workflow => {
                if let Some(workflow) = active.take() {
                    workflows.push(workflow);
                }
                workflows.push(parse_workflow(inner, current_namespace.clone())?);
            }
            Rule::workflow_decl => {
                if let Some(workflow) = active.take() {
                    workflows.push(workflow);
                }
                let (name, version, output, span) = parse_workflow_decl(inner)?;
                active = Some(Workflow {
                    name,
                    version,
                    input: None,
                    output,
                    aliases: Vec::new(),
                    namespace: current_namespace.clone(),
                    imports: Vec::new(),
                    start: None,
                    triggers: Vec::new(),
                    watches: Vec::new(),
                    type_decls: Vec::new(),
                    body: Vec::new(),
                    span,
                    leading_comments: Vec::new(),
                    dangling_comments: Vec::new(),
                });
            }
            Rule::params_block => {
                let workflow = require_active(&mut active, &inner)?;
                if workflow.input.is_some() {
                    return Err(WdlError::syntax(
                        span_of(&inner),
                        "document can only declare one params block",
                    ));
                }
                workflow.input = Some(parse_params_block(inner)?);
            }
            Rule::import_decl => {
                let workflow = require_active(&mut active, &inner)?;
                workflow.imports.push(parse_import_decl(inner)?);
            }
            Rule::trigger_decl => {
                let workflow = require_active(&mut active, &inner)?;
                workflow.triggers.push(parse_trigger_decl(inner)?);
            }
            Rule::watch_decl => {
                let workflow = require_active(&mut active, &inner)?;
                workflow.watches.push(parse_watch_decl(inner)?);
            }
            Rule::alias_decl => {
                let workflow = require_active(&mut active, &inner)?;
                workflow.aliases.push(parse_alias_decl(inner)?);
            }
            Rule::type_decl => {
                let workflow = require_active(&mut active, &inner)?;
                workflow.type_decls.push(parse_type_decl(inner)?);
            }
            Rule::start_decl => {
                let workflow = require_active(&mut active, &inner)?;
                workflow.start = Some(parse_target(first_inner(inner)?)?);
            }
            Rule::stmt => {
                let workflow = require_active(&mut active, &inner)?;
                workflow.body.push(parse_stmt(inner)?);
            }
            _ => {}
        }
    }
    if let Some(workflow) = active.take() {
        workflows.push(workflow);
    }
    if workflows.is_empty() {
        return Err(WdlError::Parse("missing workflow".into()));
    }
    let mut document = Document {
        functions,
        workflows,
        trailing_comments: Vec::new(),
    };
    // pest drops comments as silent trivia; lex them separately and attach for lossless formatting.
    attach_comments(&mut document, src);
    Ok(document)
}

/// parse a standalone WDL expression fragment.
pub fn parse_expression_fragment(src: &str) -> Result<Expr, WdlError> {
    let pair = parse_fragment_rule(src, Rule::expr_document)?;
    let expr = pair
        .into_inner()
        .find(|inner| inner.as_rule() == Rule::expr)
        .ok_or_else(|| WdlError::Parse("missing expression".into()))?;
    parse_expr(expr)
}

/// parse a standalone WDL condition fragment.
pub fn parse_condition_fragment(src: &str) -> Result<Cond, WdlError> {
    let pair = parse_fragment_rule(src, Rule::cond_document)?;
    let cond = pair
        .into_inner()
        .find(|inner| inner.as_rule() == Rule::cond)
        .ok_or_else(|| WdlError::Parse("missing condition".into()))?;
    parse_cond(cond)
}

/// parse a standalone WDL compute block fragment, including the surrounding braces.
pub fn parse_compute_fragment(src: &str) -> Result<Vec<ComputeLine>, WdlError> {
    let pair = parse_fragment_rule(src, Rule::compute_document)?;
    let block = pair
        .into_inner()
        .find(|inner| inner.as_rule() == Rule::compute_block)
        .ok_or_else(|| WdlError::Parse("missing compute block".into()))?;
    parse_compute_block(block)
}

fn parse_fragment_rule(src: &str, rule: Rule) -> Result<Pair<'_, Rule>, WdlError> {
    WdlParser::parse(rule, src)
        .map_err(|err| WdlError::Parse(err.to_string()))?
        .next()
        .ok_or_else(|| WdlError::Parse("empty input".into()))
}

fn parse_func_def(pair: Pair<Rule>) -> Result<FunctionDef, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut params = Vec::new();
    let mut ret = None;
    let mut body = None;
    let mut recursive = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::fn_recursive => {
                let depth = inner
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::integer)
                    .ok_or_else(|| WdlError::syntax(span, "@recursive requires max_depth"))?;
                let value = parse_i64(depth.as_str(), span_of(&depth))?;
                if value < 1 {
                    return Err(WdlError::syntax(
                        span,
                        "@recursive max_depth must be at least 1",
                    ));
                }
                recursive = Some(value as u32);
            }
            Rule::ident => name = inner.as_str().to_string(),
            Rule::fn_params => {
                for param in inner.into_inner().filter(|p| p.as_rule() == Rule::fn_param) {
                    params.push(parse_fn_param(param)?);
                }
            }
            Rule::type_expr => ret = Some(parse_type_expr(inner)?),
            Rule::fn_body => body = Some(parse_fn_body(inner)?),
            _ => {}
        }
    }
    let body = body.ok_or_else(|| WdlError::syntax(span, "function is missing a body"))?;
    Ok(FunctionDef {
        name,
        params,
        ret,
        body,
        recursive,
        span,
        comments: CommentSet::default(),
    })
}

/// parse a function body: a single expression, or a compute-style statement block reusing the
/// compute lines (`let`/`return`/`goto`/`if`/expr).
fn parse_fn_body(pair: Pair<Rule>) -> Result<FnBody, WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::fn_block => {
            let mut lines = Vec::new();
            for line in inner.into_inner() {
                if line.as_rule() == Rule::compute_line {
                    lines.push(parse_compute_line(line)?);
                }
            }
            Ok(FnBody::Block(lines))
        }
        Rule::expr => Ok(FnBody::Expr(Box::new(parse_expr(inner)?))),
        other => Err(WdlError::lower(format!(
            "unexpected function body {other:?}"
        ))),
    }
}

fn parse_fn_param(pair: Pair<Rule>) -> Result<FnParam, WdlError> {
    let mut name = String::new();
    let mut optional = false;
    let mut ty = TypeExpr::Named("any".into());
    let mut default = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => name = inner.as_str().to_string(),
            Rule::optional_mark => optional = true,
            Rule::type_expr => ty = parse_type_expr(inner)?,
            Rule::field_default => default = Some(parse_expr(first_inner(inner)?)?),
            _ => {}
        }
    }
    Ok(FnParam {
        name,
        ty,
        optional,
        default,
    })
}

fn span_of(pair: &Pair<Rule>) -> Span {
    let span = pair.as_span();
    Span::new(span.start(), span.end())
}

/// parse a `.wdls` secrets/config document into its declarations.
pub(crate) fn parse_secrets_document(src: &str) -> Result<Vec<SecretDecl>, WdlError> {
    let mut pairs = WdlParser::parse(Rule::secrets_document, src)
        .map_err(|err| WdlError::Parse(err.to_string()))?;
    let document = pairs
        .next()
        .ok_or_else(|| WdlError::Parse("empty input".into()))?;
    let mut decls = Vec::new();
    for inner in document.into_inner() {
        if inner.as_rule() == Rule::secret_decl {
            decls.push(parse_secret_decl(inner)?);
        }
    }
    Ok(decls)
}

fn parse_secret_decl(pair: Pair<Rule>) -> Result<SecretDecl, WdlError> {
    let span = span_of(&pair);
    let mut is_config = false;
    let mut path = Vec::new();
    let mut value = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::secret_kind => is_config = inner.as_str() == "config",
            Rule::path => {
                if let ExprKind::Path(segs) = parse_path(inner)?.kind {
                    path = segs;
                }
            }
            Rule::expr => value = Some(parse_expr(inner)?),
            _ => {}
        }
    }
    let value =
        value.ok_or_else(|| WdlError::syntax(span, "secret declaration is missing a value"))?;
    Ok(SecretDecl {
        is_config,
        path,
        value,
        span,
    })
}

pub(crate) fn parse_pipeline_document(src: &str) -> Result<Vec<PipelineDecl>, WdlError> {
    let mut pairs = WdlParser::parse(Rule::pipeline_document, src)
        .map_err(|err| WdlError::Parse(err.to_string()))?;
    let document = pairs
        .next()
        .ok_or_else(|| WdlError::Parse("empty input".into()))?;
    let mut decls = Vec::new();
    for inner in document.into_inner() {
        if inner.as_rule() == Rule::pipeline_decl {
            decls.push(parse_pipeline_decl(inner)?);
        }
    }
    Ok(decls)
}

fn parse_pipeline_decl(pair: Pair<Rule>) -> Result<PipelineDecl, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut description = None;
    let mut on_failure = None;
    let mut max_depth = None;
    let mut members = Vec::new();
    let mut links = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            // the leading `string` before `{` is the pipeline name.
            Rule::string => name = plain_string(inner)?,
            Rule::pipeline_item => {
                let item = inner
                    .into_inner()
                    .next()
                    .ok_or_else(|| WdlError::syntax(span, "empty pipeline item"))?;
                match item.as_rule() {
                    Rule::pipeline_desc => {
                        if let Some(s) = item.into_inner().find(|p| p.as_rule() == Rule::string) {
                            description = Some(plain_string(s)?);
                        }
                    }
                    Rule::pipeline_on_failure => {
                        on_failure = item
                            .into_inner()
                            .find(|p| p.as_rule() == Rule::pipeline_failure_policy)
                            .map(|p| p.as_str().to_string());
                    }
                    Rule::pipeline_max_depth => {
                        if let Some(int) = item.into_inner().find(|p| p.as_rule() == Rule::integer)
                        {
                            max_depth = Some(int.as_str().parse::<u32>().map_err(|_| {
                                WdlError::syntax(span, "max_depth must be a non-negative integer")
                            })?);
                        }
                    }
                    Rule::pipeline_member => {
                        if let Some(s) = item.into_inner().find(|p| p.as_rule() == Rule::string) {
                            members.push(plain_string(s)?);
                        }
                    }
                    Rule::pipeline_link => links.push(parse_pipeline_link(item)?),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(PipelineDecl {
        name,
        description,
        on_failure,
        max_depth,
        members,
        links,
        span,
    })
}

fn parse_pipeline_link(pair: Pair<Rule>) -> Result<PipelineLinkDecl, WdlError> {
    let span = span_of(&pair);
    let mut endpoints = Vec::with_capacity(2);
    let mut on = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => endpoints.push(plain_string(inner)?),
            Rule::pipeline_link_selector => on = Some(inner.as_str().to_string()),
            _ => {}
        }
    }
    if endpoints.len() != 2 {
        return Err(WdlError::syntax(
            span,
            "a pipeline link needs a source and a target",
        ));
    }
    let to = endpoints.pop().unwrap();
    let from = endpoints.pop().unwrap();
    Ok(PipelineLinkDecl { from, to, on, span })
}

fn parse_namespace_block(pair: Pair<Rule>) -> Result<Vec<Workflow>, WdlError> {
    let mut namespace = None;
    let mut workflows = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ns_path => namespace = Some(inner.as_str().to_string()),
            Rule::workflow => workflows.push(parse_workflow(inner, namespace.clone())?),
            _ => {}
        }
    }
    Ok(workflows)
}

fn parse_workflow(pair: Pair<Rule>, namespace: Option<String>) -> Result<Workflow, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut version = None;
    let mut input = None;
    let mut output = None;
    let mut aliases = Vec::new();
    let mut imports = Vec::new();
    let mut start = None;
    let mut triggers = Vec::new();
    let mut watches = Vec::new();
    let mut type_decls = Vec::new();
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::version => {
                let digits = inner.as_str().trim_start_matches('v');
                version =
                    Some(digits.parse::<SemVer>().map_err(|err| {
                        WdlError::syntax(span, format!("invalid version: {err}"))
                    })?);
            }
            Rule::returns_decl => {
                let ty = inner
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::type_expr)
                    .ok_or_else(|| {
                        WdlError::syntax(span, "returns declaration is missing a type")
                    })?;
                output = Some(parse_type_expr(ty)?);
            }
            Rule::params_block => input = Some(parse_params_block(inner)?),
            Rule::import_decl => imports.push(parse_import_decl(inner)?),
            Rule::trigger_decl => triggers.push(parse_trigger_decl(inner)?),
            Rule::watch_decl => watches.push(parse_watch_decl(inner)?),
            Rule::alias_decl => aliases.push(parse_alias_decl(inner)?),
            Rule::type_decl => type_decls.push(parse_type_decl(inner)?),
            Rule::start_decl => start = Some(parse_target(first_inner(inner)?)?),
            Rule::stmt => body.push(parse_stmt(inner)?),
            _ => {}
        }
    }
    Ok(Workflow {
        name,
        version,
        input,
        output,
        aliases,
        namespace,
        imports,
        start,
        triggers,
        watches,
        type_decls,
        body,
        span,
        leading_comments: Vec::new(),
        dangling_comments: Vec::new(),
    })
}

fn parse_workflow_decl(
    pair: Pair<Rule>,
) -> Result<(String, Option<SemVer>, Option<TypeExpr>, Span), WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut version = None;
    let mut output = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::version => {
                let digits = inner.as_str().trim_start_matches('v');
                version =
                    Some(digits.parse::<SemVer>().map_err(|err| {
                        WdlError::syntax(span, format!("invalid version: {err}"))
                    })?);
            }
            Rule::returns_decl => {
                let ty = inner
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::type_expr)
                    .ok_or_else(|| {
                        WdlError::syntax(span, "returns declaration is missing a type")
                    })?;
                output = Some(parse_type_expr(ty)?);
            }
            _ => {}
        }
    }
    Ok((name, version, output, span))
}

fn parse_watch_decl(pair: Pair<Rule>) -> Result<WatchDecl, WdlError> {
    let mut cond = None;
    let mut handler = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::cond => cond = Some(parse_cond(inner)?),
            Rule::target => handler = Some(parse_target(inner)?),
            _ => {}
        }
    }
    Ok(WatchDecl {
        cond: cond.ok_or_else(|| WdlError::lower("watch missing condition"))?,
        handler: handler.ok_or_else(|| WdlError::lower("watch missing handler target"))?,
    })
}

fn parse_import_decl(pair: Pair<Rule>) -> Result<Import, WdlError> {
    let span = span_of(&pair);
    let mut path = String::new();
    let mut alias = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ns_path => path = inner.as_str().to_string(),
            Rule::ident => alias = Some(inner.as_str().to_string()),
            _ => {}
        }
    }
    Ok(Import {
        path,
        alias,
        span,
        comments: CommentSet::default(),
    })
}

fn parse_trigger_decl(pair: Pair<Rule>) -> Result<TriggerDecl, WdlError> {
    let span = span_of(&pair);
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::cron_trigger => parse_cron_trigger(inner, span),
        Rule::chained_trigger => parse_chained_trigger(inner, span),
        _ => Err(WdlError::syntax(span, "unrecognized trigger declaration")),
    }
}

fn parse_cron_trigger(pair: Pair<Rule>, span: Span) -> Result<TriggerDecl, WdlError> {
    let mut schedule = None;
    let mut params = None;
    let mut enabled = true;
    let mut blackout_start = None;
    let mut blackout_end = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => schedule = Some(parse_expr(inner)?),
            Rule::object => params = Some(parse_object(inner)?),
            Rule::trigger_option => {
                let option = first_inner(inner)?;
                match option.as_rule() {
                    Rule::trigger_disabled => enabled = false,
                    Rule::trigger_blackout => {
                        let mut exprs = option
                            .into_inner()
                            .filter(|part| part.as_rule() == Rule::expr);
                        blackout_start = exprs.next().map(parse_expr).transpose()?;
                        blackout_end = exprs.next().map(parse_expr).transpose()?;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let schedule =
        schedule.ok_or_else(|| WdlError::syntax(span, "trigger is missing a cron expression"))?;
    Ok(TriggerDecl {
        kind: TriggerDeclKind::Cron {
            schedule,
            blackout_start,
            blackout_end,
        },
        params,
        enabled,
        span,
        comments: CommentSet::default(),
    })
}

fn parse_chained_trigger(pair: Pair<Rule>, span: Span) -> Result<TriggerDecl, WdlError> {
    let mut event = None;
    let mut target = None;
    let mut params = None;
    let mut enabled = true;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::chain_event => {
                event = Some(match inner.as_str() {
                    "on_failure" => ChainEvent::Failure,
                    "on_complete" => ChainEvent::Complete,
                    _ => ChainEvent::Success,
                });
            }
            Rule::expr => target = Some(parse_expr(inner)?),
            Rule::object => params = Some(parse_object(inner)?),
            Rule::trigger_disabled => enabled = false,
            _ => {}
        }
    }
    let event =
        event.ok_or_else(|| WdlError::syntax(span, "chained trigger is missing an event"))?;
    let target = target
        .ok_or_else(|| WdlError::syntax(span, "chained trigger is missing a target workflow"))?;
    Ok(TriggerDecl {
        kind: TriggerDeclKind::Chained { event, target },
        params,
        enabled,
        span,
        comments: CommentSet::default(),
    })
}

fn parse_alias_decl(pair: Pair<Rule>) -> Result<Alias, WdlError> {
    let span = span_of(&pair);
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .ok_or_else(|| WdlError::lower("alias name"))?
        .as_str()
        .to_string();
    let object = inner.next().ok_or_else(|| WdlError::lower("alias body"))?;
    let entries = parse_object_entries(object)?;
    Ok(Alias {
        name,
        entries,
        span,
        comments: CommentSet::default(),
    })
}

// parameter typing -----------------------------------------------------------

fn parse_params_block(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let mut fields = Vec::new();
    let mut additional = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::params_field => fields.push(parse_params_field(inner)?),
            Rule::type_additional => {
                let ty = inner
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::type_expr)
                    .ok_or_else(|| WdlError::lower("open parameter additional type"))?;
                additional = Some(Box::new(parse_type_expr(ty)?));
            }
            _ => {}
        }
    }
    Ok(TypeExpr::Struct { fields, additional })
}

/// a top-level workflow parameter field, optionally carrying a `= expr` default.
fn parse_params_field(pair: Pair<Rule>) -> Result<TypeField, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut optional = false;
    let mut ty = TypeExpr::Named("any".into());
    let mut default = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => name = inner.as_str().to_string(),
            Rule::string => name = plain_string(inner)?,
            Rule::optional_mark => optional = true,
            Rule::type_expr => ty = parse_type_expr(inner)?,
            Rule::field_default => default = Some(parse_expr(first_inner(inner)?)?),
            _ => {}
        }
    }
    Ok(TypeField {
        name,
        optional,
        ty,
        default,
        span,
        comments: CommentSet::default(),
    })
}

fn parse_type_field(pair: Pair<Rule>) -> Result<TypeField, WdlError> {
    let span = span_of(&pair);
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
    Ok(TypeField {
        name,
        optional,
        ty,
        default: None,
        span,
        comments: CommentSet::default(),
    })
}

fn parse_type_expr(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    // type_expr -> type_union
    let union = first_inner(pair)?;
    parse_type_union(union)
}

fn parse_type_union(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let mut variants = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::type_range)
        .map(parse_type_range)
        .collect::<Result<Vec<_>, _>>()?;
    if variants.len() == 1 {
        return Ok(variants.remove(0));
    }
    Ok(TypeExpr::Union(variants))
}

fn parse_type_range(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let span = span_of(&pair);
    let mut inner = pair.into_inner();
    let base = inner
        .next()
        .ok_or_else(|| WdlError::syntax(span, "range type is missing a base type"))
        .and_then(parse_type_postfix)?;
    let Some(bounds_pair) = inner.find(|p| p.as_rule() == Rule::range_bounds) else {
        return Ok(base);
    };
    let mut min = None;
    let mut max = None;
    for bound in bounds_pair.into_inner() {
        match bound.as_rule() {
            Rule::range_min => min = Some(parse_type_bound(first_inner(bound)?)?),
            Rule::range_max => max = Some(parse_type_bound(first_inner(bound)?)?),
            _ => {}
        }
    }
    Ok(TypeExpr::Range {
        base: Box::new(base),
        min,
        max,
    })
}

fn parse_type_bound(pair: Pair<Rule>) -> Result<Value, WdlError> {
    let span = span_of(&pair);
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::duration => Ok(Value::from(parse_duration(inner.as_str(), span)?)),
        Rule::number if inner.as_str().contains('.') => {
            let value = inner
                .as_str()
                .parse::<f64>()
                .map_err(|_| WdlError::syntax(span, "invalid range bound"))?;
            Ok(Value::from(value))
        }
        Rule::number => Ok(Value::from(parse_i64(inner.as_str(), span)?)),
        _ => Err(WdlError::syntax(span, "invalid range bound")),
    }
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
        Rule::type_enum => parse_type_enum(inner),
        Rule::type_function => parse_type_function(inner),
        Rule::type_struct => parse_type_struct(inner),
        Rule::type_named => Ok(TypeExpr::Named(inner.as_str().to_string())),
        other => Err(WdlError::lower(format!("unexpected type atom {other:?}"))),
    }
}

fn parse_type_function(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    // type_function -> "function" "<" "(" (type_expr ("," type_expr)*)? ")" "->" type_expr ">".
    // the trailing type_expr is the return; every earlier one is a parameter.
    let mut types = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::type_expr)
        .map(parse_type_expr)
        .collect::<Result<Vec<_>, _>>()?;
    let ret = types
        .pop()
        .ok_or_else(|| WdlError::lower("function type is missing a return type"))?;
    Ok(TypeExpr::Function {
        params: types,
        ret: Box::new(ret),
    })
}

fn parse_type_enum(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let values = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::enum_lit)
        .map(parse_enum_lit)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TypeExpr::Enum(values))
}

fn parse_enum_lit(pair: Pair<Rule>) -> Result<Value, WdlError> {
    let span = span_of(&pair);
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::string => Ok(Value::String(plain_string(inner)?)),
        Rule::number if inner.as_str().contains('.') => {
            let value = inner
                .as_str()
                .parse::<f64>()
                .map_err(|_| WdlError::syntax(span, "invalid enum number"))?;
            Ok(Value::from(value))
        }
        Rule::number => Ok(Value::from(parse_i64(inner.as_str(), span)?)),
        Rule::boolean => Ok(Value::Bool(inner.as_str() == "true")),
        Rule::null_lit => Ok(Value::Null),
        _ => Err(WdlError::syntax(span, "invalid enum literal")),
    }
}

fn parse_type_struct(pair: Pair<Rule>) -> Result<TypeExpr, WdlError> {
    let mut fields = Vec::new();
    let mut additional = None;
    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::type_field => fields.push(parse_type_field(part)?),
            Rule::type_additional => {
                let ty = part
                    .into_inner()
                    .find(|p| p.as_rule() == Rule::type_expr)
                    .ok_or_else(|| WdlError::lower("open struct additional type"))?;
                additional = Some(Box::new(parse_type_expr(ty)?));
            }
            _ => {}
        }
    }
    Ok(TypeExpr::Struct { fields, additional })
}

fn parse_type_decl(pair: Pair<Rule>) -> Result<TypeDecl, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut ty = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::type_named => name = inner.as_str().to_string(),
            Rule::type_struct => ty = Some(parse_type_struct(inner)?),
            Rule::type_expr => ty = Some(parse_type_expr(inner)?),
            _ => {}
        }
    }
    let ty = ty.ok_or_else(|| WdlError::syntax(span, "type declaration is missing a body"))?;
    Ok(TypeDecl {
        name,
        ty,
        span,
        comments: CommentSet::default(),
    })
}

// statements ----------------------------------------------------------------

fn parse_stmt(pair: Pair<Rule>) -> Result<Stmt, WdlError> {
    let span = span_of(&pair);
    let mut annotations = Annotations::default();
    let mut label = None;
    let mut label_type = None;
    let mut kind = None;
    let mut transitions = TransitionClause::default();
    let mut compensation = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::annotation => apply_annotation(&mut annotations, inner)?,
            Rule::node_decl => {
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::ident => label = Some(part.as_str().to_string()),
                        Rule::type_expr => label_type = Some(parse_type_expr(part)?),
                        _ => {}
                    }
                }
            }
            Rule::stmt_body => kind = Some(parse_stmt_body(inner)?),
            Rule::compensate_clause => {
                compensation = Some(Box::new(parse_action(first_inner(inner)?)?));
            }
            Rule::transitions => transitions = parse_transitions(inner)?,
            _ => {}
        }
    }
    let kind = kind.ok_or_else(|| WdlError::syntax(span, "statement has no body"))?;
    if matches!(kind, StmtKind::Yield(_)) && label.is_some() {
        return Err(WdlError::syntax(
            span,
            "`yield` cannot be bound with `node`",
        ));
    }
    if matches!(kind, StmtKind::Yield(_)) && !transitions.is_empty() {
        return Err(WdlError::syntax(span, "`yield` cannot declare transitions"));
    }
    Ok(Stmt {
        span,
        annotations,
        label,
        label_type,
        kind,
        transitions,
        compensation,
        comments: CommentSet::default(),
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
        Rule::ann_lock => annotations.locked = true,
        Rule::ann_timeout => {
            let duration = first_inner(inner)?;
            annotations.timeout_seconds =
                Some(parse_duration(duration.as_str(), span_of(&duration))?);
        }
        _ => {}
    }
    Ok(())
}

fn parse_stmt_body(pair: Pair<Rule>) -> Result<StmtKind, WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::action_stmt => Ok(StmtKind::Action(parse_action(inner)?)),
        Rule::compute_stmt => Ok(StmtKind::Compute(parse_compute(inner)?)),
        Rule::subflow_stmt => Ok(StmtKind::Subflow(parse_subflow(inner)?)),
        Rule::wait_cond_stmt => Ok(StmtKind::While(parse_wait_until(inner)?)),
        Rule::wait_stmt => Ok(StmtKind::Wait(parse_wait(inner)?)),
        Rule::output_stmt => Ok(StmtKind::Output(parse_output_block(inner)?)),
        Rule::emit_stmt => Ok(StmtKind::Output(parse_output(inner)?)),
        Rule::yield_stmt => Ok(StmtKind::Yield(parse_expr(first_inner(inner)?)?)),
        Rule::input_stmt => Ok(StmtKind::Input(parse_input(inner)?)),
        Rule::approval_stmt => Ok(StmtKind::Approval(parse_approval(inner)?)),
        Rule::gate_stmt => Ok(StmtKind::Gate(parse_gate(inner)?)),
        Rule::signal_stmt => Ok(StmtKind::Signal(parse_signal(inner)?)),
        Rule::assert_stmt => Ok(StmtKind::Assert(parse_assert(inner)?)),
        Rule::transform_stmt => Ok(StmtKind::Transform(parse_transform(inner)?)),
        Rule::audit_stmt => Ok(StmtKind::Audit(parse_audit(inner)?)),
        Rule::checkpoint_stmt => Ok(StmtKind::Checkpoint(parse_checkpoint(inner)?)),
        Rule::mutex_stmt => Ok(StmtKind::Mutex(parse_mutex(inner)?)),
        Rule::throttle_stmt => Ok(StmtKind::Throttle(parse_throttle(inner)?)),
        Rule::await_stmt => Ok(StmtKind::Await(parse_await(inner)?)),
        Rule::debounce_stmt => Ok(StmtKind::Debounce(parse_debounce(inner)?)),
        Rule::collect_stmt => Ok(StmtKind::Collect(parse_collect(inner)?)),
        Rule::barrier_stmt => Ok(StmtKind::Barrier(parse_barrier(inner)?)),
        Rule::circuit_breaker_stmt => Ok(StmtKind::CircuitBreaker(parse_circuit_breaker(inner)?)),
        Rule::event_source_stmt => Ok(StmtKind::EventSource(parse_event_source(inner)?)),
        Rule::config_stmt => Ok(StmtKind::Config(parse_config(inner)?)),
        Rule::fail_stmt => Ok(StmtKind::Fail(parse_fail(inner)?)),
        Rule::if_stmt => Ok(StmtKind::If(parse_if(inner)?)),
        Rule::for_stmt => Ok(StmtKind::For(parse_for(inner)?)),
        Rule::while_stmt => Ok(StmtKind::While(parse_while(inner, false)?)),
        Rule::until_stmt => Ok(StmtKind::While(parse_while(inner, true)?)),
        Rule::match_stmt => Ok(StmtKind::Match(parse_match(inner)?)),
        Rule::toggle_stmt => Ok(StmtKind::Match(parse_toggle(inner)?)),
        Rule::split_stmt => Ok(StmtKind::Match(parse_split(inner)?)),
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
    if idents.len() < 2 {
        return Err(WdlError::syntax(
            span,
            "action requires provider.function (provider may be a dotted namespace path)",
        ));
    }
    // the trailing segment is the function; the leading segments are the provider namespace path.
    let function = idents
        .pop()
        .ok_or_else(|| WdlError::lower("action function"))?;
    let provider = idents.join(".");
    Ok(ActionStmt {
        provider,
        function,
        args,
        modifiers,
    })
}

fn parse_compute(pair: Pair<Rule>) -> Result<ComputeStmt, WdlError> {
    let mut body = Vec::new();
    let mut foreign = None;
    let mut modifiers = Modifiers::default();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::compute_block => body = parse_compute_block(inner)?,
            Rule::foreign_compute => foreign = Some(parse_foreign_compute(inner)?),
            Rule::modifier => apply_modifier(&mut modifiers, inner)?,
            _ => {}
        }
    }
    Ok(ComputeStmt {
        body,
        foreign,
        modifiers,
    })
}

fn parse_foreign_compute(pair: Pair<Rule>) -> Result<ForeignCompute, WdlError> {
    let mut language = None;
    let mut source = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => language = Some(plain_string(inner)?),
            Rule::raw_block => source = Some(raw_block_content(inner.as_str())),
            _ => {}
        }
    }
    Ok(ForeignCompute {
        language: language.unwrap_or_default(),
        source: source.unwrap_or_default(),
    })
}

fn parse_compute_block(pair: Pair<Rule>) -> Result<Vec<ComputeLine>, WdlError> {
    let mut lines = Vec::new();
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::compute_line {
            lines.push(parse_compute_line(inner)?);
        }
    }
    Ok(lines)
}

fn parse_compute_line(pair: Pair<Rule>) -> Result<ComputeLine, WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::compute_let => {
            let mut name = String::new();
            let mut ty = None;
            let mut value = None;
            for part in inner.into_inner() {
                match part.as_rule() {
                    Rule::ident => name = part.as_str().to_string(),
                    Rule::type_expr => ty = Some(parse_type_expr(part)?),
                    Rule::expr => value = Some(parse_expr(part)?),
                    _ => {}
                }
            }
            let value = value.ok_or_else(|| WdlError::lower("compute let missing value"))?;
            Ok(ComputeLine::Let { name, ty, value })
        }
        Rule::compute_return => Ok(ComputeLine::Return(parse_expr(first_inner(inner)?)?)),
        Rule::compute_goto => Ok(ComputeLine::Goto(parse_target(first_inner(inner)?)?)),
        Rule::compute_if => {
            let mut cond = None;
            let mut blocks = Vec::new();
            for part in inner.into_inner() {
                match part.as_rule() {
                    Rule::cond => cond = Some(parse_cond(part)?),
                    Rule::compute_block => blocks.push(parse_compute_block(part)?),
                    _ => {}
                }
            }
            let cond = cond.ok_or_else(|| WdlError::lower("compute if missing condition"))?;
            let mut blocks = blocks.into_iter();
            let then_branch = blocks.next().unwrap_or_default();
            let else_branch = blocks.next().unwrap_or_default();
            Ok(ComputeLine::If {
                cond,
                then_branch,
                else_branch,
            })
        }
        Rule::compute_expr_stmt => Ok(ComputeLine::Expr(parse_expr(first_inner(inner)?)?)),
        other => Err(WdlError::lower(format!(
            "unexpected compute line {other:?}"
        ))),
    }
}

// a lambda `params => body`; params are bare identifiers (`x` or `(a, b)`), body is any expr.
fn parse_lambda(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut params = Vec::new();
    let mut body = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::lambda_params => {
                params = inner
                    .into_inner()
                    .filter(|p| p.as_rule() == Rule::ident)
                    .map(|p| p.as_str().to_string())
                    .collect();
            }
            Rule::expr => body = Some(parse_expr(inner)?),
            _ => {}
        }
    }
    let body = body.ok_or_else(|| WdlError::lower("lambda missing body"))?;
    Ok(Expr::new(
        ExprKind::Lambda {
            params,
            body: Box::new(body),
        },
        span,
    ))
}

// a library call `name(args...)`. `string`/`json` are excluded by the grammar; they stay coercions.
fn parse_lib_call(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut name = String::new();
    let mut args = Vec::new();
    let mut named = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::ident => name = inner.as_str().to_string(),
            Rule::call_arg => parse_call_arg(inner, &mut args, &mut named)?,
            _ => {}
        }
    }
    // a prefix `name(...)` call: subject to the std-qualification requirement during resolution.
    Ok(Expr::new(
        ExprKind::Call {
            name,
            args,
            named,
            method: false,
        },
        span,
    ))
}

// split one `call_arg` into the positional or keyword bucket.
fn parse_call_arg(
    pair: Pair<Rule>,
    args: &mut Vec<Expr>,
    named: &mut Vec<(String, Expr)>,
) -> Result<(), WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::named_arg => {
            let mut parts = inner.into_inner();
            let key = parts
                .next()
                .ok_or_else(|| WdlError::lower("named argument missing name"))?;
            let value = parts
                .next()
                .ok_or_else(|| WdlError::lower("named argument missing value"))?;
            named.push((key.as_str().to_string(), parse_expr(value)?));
        }
        _ => args.push(parse_expr(inner)?),
    }
    Ok(())
}

fn parse_condition_expr(expr: Expr, span: Span) -> Result<Expr, WdlError> {
    if matches!(expr.kind, ExprKind::Lambda { .. }) {
        return Err(WdlError::syntax(
            span,
            "condition expressions cannot be lambdas",
        ));
    }
    Ok(expr)
}

// argument entries in source order; a `...alias` spread becomes an entry with an `ExprKind::Spread`
// value (the key is unused), expanded later by desugaring.
fn parse_arg_list(pair: Pair<Rule>) -> Result<Vec<(String, Expr)>, WdlError> {
    let mut args = Vec::new();
    for arg in pair.into_inner().filter(|p| p.as_rule() == Rule::arg) {
        let entry = arg
            .into_inner()
            .next()
            .ok_or_else(|| WdlError::lower("arg entry"))?;
        match entry.as_rule() {
            Rule::arg_spread => args.push(parse_spread_entry(entry)?),
            Rule::arg_pair => {
                let mut inner = entry.into_inner();
                let name = inner.next().ok_or_else(|| WdlError::lower("arg name"))?;
                let value = inner.next().ok_or_else(|| WdlError::lower("arg value"))?;
                args.push((name.as_str().to_string(), parse_expr(value)?));
            }
            _ => {}
        }
    }
    Ok(args)
}

// build a spread entry from an `arg_spread`/`object_spread` pair: an empty key paired with an
// `ExprKind::Spread` value carrying the alias name and the spread's source span.
fn parse_spread_entry(pair: Pair<Rule>) -> Result<(String, Expr), WdlError> {
    let span = span_of(&pair);
    let name = pair
        .into_inner()
        .next()
        .ok_or_else(|| WdlError::lower("spread alias"))?
        .as_str()
        .to_string();
    Ok((String::new(), Expr::new(ExprKind::Spread(name), span)))
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
            let find = |key: &str| named.iter().find(|(k, _)| k == key).map(|(_, v)| v);
            let backoff_base_seconds = find("backoff")
                .map(|v| expect_int(Some(v), "retry backoff"))
                .transpose()?;
            let backoff_max_seconds = find("max")
                .map(|v| expect_int(Some(v), "retry max"))
                .transpose()?;
            let jitter = match find("jitter") {
                Some(value) => expect_bool(value, "retry jitter")?,
                None => false,
            };
            let retry_on = find("on")
                .map(|v| expect_ident_or_string(v, "retry on"))
                .transpose()?;
            modifiers.retry = Some(crate::ast::RetryConfig {
                max_attempts: expect_int(positional.first(), "retry")?,
                backoff_base_seconds,
                backoff_max_seconds,
                jitter,
                retry_on,
            });
        }
        "tags" => {
            for value in &positional {
                modifiers.tags.push(expect_string(value, "tags")?);
            }
        }
        "mcp" => modifiers.mcp = true,
        "runner" => {
            let value = positional
                .first()
                .ok_or_else(|| WdlError::syntax(span, "runner requires a string argument"))?;
            modifiers.runner = Some(expect_string(value, "runner")?);
        }
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
            Rule::string => workflow_name = plain_string(inner)?,
            Rule::subflow_arg => {
                let span = span_of(&inner);
                let mut parts = inner.into_inner();
                let key = parts
                    .next()
                    .ok_or_else(|| WdlError::lower("subflow argument missing name"))?
                    .as_str()
                    .to_string();
                let value_pair = parts
                    .next()
                    .ok_or_else(|| WdlError::lower("subflow argument missing value"))?;
                match key.as_str() {
                    "params" => {
                        let expr = parse_expr(value_pair)?;
                        let ExprKind::Object(entries) = expr.kind else {
                            return Err(WdlError::syntax(
                                span,
                                "subflow params must be an object literal",
                            ));
                        };
                        params = entries;
                    }
                    "detached" => detached = expect_bool_expr(parse_expr(value_pair)?, "detached")?,
                    "reuse" => reuse = expect_bool_expr(parse_expr(value_pair)?, "reuse")?,
                    "name" => run_name = Some(parse_expr(value_pair)?),
                    other => {
                        return Err(WdlError::syntax(
                            span,
                            format!("unknown subflow argument '{other}'"),
                        ));
                    }
                }
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

fn expect_bool_expr(expr: Expr, name: &str) -> Result<bool, WdlError> {
    match expr.kind {
        ExprKind::Bool(value) => Ok(value),
        _ => Err(WdlError::syntax(
            expr.span,
            format!("subflow {name} must be a boolean literal"),
        )),
    }
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

fn parse_output(pair: Pair<Rule>) -> Result<OutputStmt, WdlError> {
    let mut event_type = None;
    let mut data = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => event_type = Some(plain_string(inner)?),
            Rule::expr => data = Some(parse_expr(inner)?),
            _ => {}
        }
    }
    Ok(OutputStmt {
        event_type,
        data,
        items: Vec::new(),
    })
}

fn parse_output_block(pair: Pair<Rule>) -> Result<OutputStmt, WdlError> {
    let mut event_type = None;
    let mut data = None;
    let mut items = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::emit_stmt => {
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::string => event_type = Some(plain_string(part)?),
                        Rule::expr => data = Some(parse_expr(part)?),
                        _ => {}
                    }
                }
            }
            Rule::artifact_line => {
                let mut name = String::new();
                let mut source = None;
                for part in inner.into_inner() {
                    match part.as_rule() {
                        Rule::ident => name = part.as_str().to_string(),
                        Rule::expr => source = Some(parse_expr(part)?),
                        _ => {}
                    }
                }
                if let Some(source) = source {
                    items.push((name, source));
                }
            }
            _ => {}
        }
    }
    Ok(OutputStmt {
        event_type,
        data,
        items,
    })
}

fn parse_input(pair: Pair<Rule>) -> Result<InputStmt, WdlError> {
    let prompt = pair
        .into_inner()
        .find(|inner| inner.as_rule() == Rule::expr)
        .map(parse_expr)
        .transpose()?;
    Ok(InputStmt { prompt })
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

fn parse_gate(pair: Pair<Rule>) -> Result<GateStmt, WdlError> {
    let mut kind = "manual".to_string();
    let mut when = None;
    let mut poll_interval = None;
    let mut timeout = None;
    let mut metadata = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::gate_kind => kind = inner.as_str().trim().to_string(),
            Rule::gate_when => when = Some(parse_cond(first_inner(inner)?)?),
            Rule::gate_every => {
                let value = first_inner(inner)?;
                poll_interval = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::gate_timeout => {
                let value = first_inner(inner)?;
                timeout = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::object => metadata = parse_object_entries(inner)?,
            _ => {}
        }
    }
    Ok(GateStmt {
        kind,
        when,
        poll_interval,
        timeout,
        metadata,
    })
}

fn parse_signal(pair: Pair<Rule>) -> Result<SignalStmt, WdlError> {
    let mut name = String::new();
    let mut correlation_key = None;
    let mut metadata = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::signal_key => correlation_key = Some(parse_expr(first_inner(inner)?)?),
            Rule::object => metadata = parse_object_entries(inner)?,
            _ => {}
        }
    }
    Ok(SignalStmt {
        name,
        correlation_key,
        metadata,
    })
}

fn parse_assert(pair: Pair<Rule>) -> Result<AssertStmt, WdlError> {
    let mut assertions = Vec::new();
    for line in pair.into_inner() {
        if line.as_rule() != Rule::assert_line {
            continue;
        }
        let mut name = String::new();
        let mut cond = None;
        for inner in line.into_inner() {
            match inner.as_rule() {
                Rule::string => name = plain_string(inner)?,
                Rule::cond => cond = Some(parse_cond(inner)?),
                _ => {}
            }
        }
        let cond = cond.ok_or_else(|| WdlError::lower("assert line missing condition"))?;
        assertions.push((name, cond));
    }
    Ok(AssertStmt { assertions })
}

fn parse_transform(pair: Pair<Rule>) -> Result<TransformStmt, WdlError> {
    let mut bindings = Vec::new();
    for line in pair.into_inner() {
        if line.as_rule() != Rule::transform_line {
            continue;
        }
        let mut name = String::new();
        let mut value = None;
        for inner in line.into_inner() {
            match inner.as_rule() {
                Rule::ident => name = inner.as_str().to_string(),
                Rule::expr => value = Some(parse_expr(inner)?),
                _ => {}
            }
        }
        let value = value.ok_or_else(|| WdlError::lower("transform line missing value"))?;
        bindings.push((name, value));
    }
    Ok(TransformStmt { bindings })
}

fn parse_audit(pair: Pair<Rule>) -> Result<AuditStmt, WdlError> {
    let mut action = None;
    let mut actor = None;
    let mut target = None;
    let mut reason = None;
    for field in pair.into_inner() {
        if field.as_rule() != Rule::audit_field {
            continue;
        }
        let mut kw = String::new();
        let mut value = None;
        for inner in field.into_inner() {
            match inner.as_rule() {
                Rule::audit_kw => kw = inner.as_str().trim().to_string(),
                Rule::expr => value = Some(parse_expr(inner)?),
                _ => {}
            }
        }
        let value = value.ok_or_else(|| WdlError::lower("audit field missing value"))?;
        match kw.as_str() {
            "action" => action = Some(value),
            "actor" => actor = Some(value),
            "target" => target = Some(value),
            "reason" => reason = Some(value),
            _ => {}
        }
    }
    let action = action.ok_or_else(|| WdlError::lower("audit requires an action"))?;
    Ok(AuditStmt {
        action,
        actor,
        target,
        reason,
    })
}

fn parse_checkpoint(pair: Pair<Rule>) -> Result<CheckpointStmt, WdlError> {
    let name = plain_string(first_inner(pair)?)?;
    Ok(CheckpointStmt { name })
}

fn parse_mutex(pair: Pair<Rule>) -> Result<MutexStmt, WdlError> {
    let mut name = String::new();
    let mut poll_interval = None;
    let mut timeout = None;
    let mut hold = None;
    let mut release = false;
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::mutex_release => release = true,
            Rule::string => name = plain_string(inner)?,
            Rule::poll_every => {
                let value = first_inner(inner)?;
                poll_interval = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::node_timeout => {
                let value = first_inner(inner)?;
                timeout = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::mutex_hold => {
                let value = first_inner(inner)?;
                hold = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::block => body = parse_block(inner)?,
            _ => {}
        }
    }
    Ok(MutexStmt {
        name,
        poll_interval,
        timeout,
        hold,
        release,
        body,
    })
}

fn parse_throttle(pair: Pair<Rule>) -> Result<ThrottleStmt, WdlError> {
    let mut name = String::new();
    let mut max_per_window = 0;
    let mut window_seconds = 0;
    let mut poll_interval = None;
    let mut timeout = None;
    // the rate integer and the `per` duration arrive in order; the first integer is the rate.
    let mut seen_rate = false;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::integer if !seen_rate => {
                max_per_window = parse_i64(inner.as_str(), span_of(&inner))?;
                seen_rate = true;
            }
            Rule::duration => {
                window_seconds = parse_duration(inner.as_str(), span_of(&inner))?;
            }
            Rule::poll_every => {
                let value = first_inner(inner)?;
                poll_interval = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::node_timeout => {
                let value = first_inner(inner)?;
                timeout = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            _ => {}
        }
    }
    Ok(ThrottleStmt {
        name,
        max_per_window,
        window_seconds,
        poll_interval,
        timeout,
    })
}

fn parse_await(pair: Pair<Rule>) -> Result<AwaitStmt, WdlError> {
    let mut run_ids = None;
    let mut mode = None;
    let mut poll_interval = None;
    let mut timeout = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => run_ids = Some(parse_expr(inner)?),
            Rule::await_mode => mode = Some(plain_string(first_inner(inner)?)?),
            Rule::poll_every => {
                let value = first_inner(inner)?;
                poll_interval = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::node_timeout => {
                let value = first_inner(inner)?;
                timeout = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            _ => {}
        }
    }
    let run_ids = run_ids.ok_or_else(|| WdlError::lower("await requires a run-id expression"))?;
    Ok(AwaitStmt {
        run_ids,
        mode,
        poll_interval,
        timeout,
    })
}

fn parse_debounce(pair: Pair<Rule>) -> Result<DebounceStmt, WdlError> {
    let mut name = String::new();
    let mut delay_seconds = 0;
    let mut key = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::duration => {
                delay_seconds = parse_duration(inner.as_str(), span_of(&inner))?;
            }
            Rule::debounce_key => key = Some(parse_expr(first_inner(inner)?)?),
            _ => {}
        }
    }
    Ok(DebounceStmt {
        name,
        delay_seconds,
        key,
    })
}

fn parse_collect(pair: Pair<Rule>) -> Result<CollectStmt, WdlError> {
    let mut name = String::new();
    let mut max = 0;
    let mut timeout = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::integer => max = parse_i64(inner.as_str(), span_of(&inner))?,
            Rule::node_timeout => {
                let value = first_inner(inner)?;
                timeout = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            _ => {}
        }
    }
    Ok(CollectStmt { name, max, timeout })
}

fn parse_barrier(pair: Pair<Rule>) -> Result<BarrierStmt, WdlError> {
    let mut name = String::new();
    let mut count = 0;
    let mut poll_interval = None;
    let mut timeout = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::integer => count = parse_i64(inner.as_str(), span_of(&inner))?,
            Rule::poll_every => {
                let value = first_inner(inner)?;
                poll_interval = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            Rule::node_timeout => {
                let value = first_inner(inner)?;
                timeout = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            _ => {}
        }
    }
    Ok(BarrierStmt {
        name,
        count,
        poll_interval,
        timeout,
    })
}

fn parse_circuit_breaker(pair: Pair<Rule>) -> Result<CircuitBreakerStmt, WdlError> {
    let mut name = String::new();
    let mut threshold = 0;
    // the window duration comes before the cooldown duration in source order.
    let mut durations = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => name = plain_string(inner)?,
            Rule::integer => threshold = parse_i64(inner.as_str(), span_of(&inner))?,
            Rule::duration => {
                durations.push(parse_duration(inner.as_str(), span_of(&inner))?);
            }
            _ => {}
        }
    }
    let window_seconds = durations.first().copied().unwrap_or(60);
    let cooldown_seconds = durations.get(1).copied().unwrap_or(120);
    Ok(CircuitBreakerStmt {
        name,
        threshold,
        window_seconds,
        cooldown_seconds,
    })
}

fn parse_event_source(pair: Pair<Rule>) -> Result<EventSourceStmt, WdlError> {
    let mut event_type = String::new();
    let mut filter = None;
    let mut max = None;
    let mut timeout = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => event_type = plain_string(inner)?,
            Rule::event_filter => filter = Some(parse_cond(first_inner(inner)?)?),
            Rule::event_max => {
                let value = first_inner(inner)?;
                max = Some(parse_i64(value.as_str(), span_of(&value))?);
            }
            Rule::node_timeout => {
                let value = first_inner(inner)?;
                timeout = Some(parse_duration(value.as_str(), span_of(&value))?);
            }
            _ => {}
        }
    }
    Ok(EventSourceStmt {
        event_type,
        filter,
        max,
        timeout,
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
            // `limit none` yields no expr child and means uncapped (limit stays None);
            // `limit <expr>` carries an integer-valued expression.
            Rule::for_limit_expr => {
                for child in inner.into_inner() {
                    if child.as_rule() == Rule::expr {
                        limit = Some(parse_expr(child)?);
                    }
                }
            }
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

/// desugar `wait until <cond> [every <dur>]` into `until <cond> { wait <interval> }`. the condition
/// wait shares the loop runtime (a reentry-enabled condition node with a back-edge), so no new node
/// kind or reducer path is needed; it is purely a terser surface for a bodyless poll wait.
fn parse_wait_until(pair: Pair<Rule>) -> Result<WhileStmt, WdlError> {
    const DEFAULT_WAIT_UNTIL_INTERVAL_SECONDS: i64 = 30;
    let mut cond = None;
    let mut interval = DEFAULT_WAIT_UNTIL_INTERVAL_SECONDS;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::cond => cond = Some(parse_cond(inner)?),
            Rule::wait_every => {
                let duration = first_inner(inner)?;
                interval = parse_duration(duration.as_str(), span_of(&duration))?;
            }
            _ => {}
        }
    }
    let wait = Stmt {
        span: Span::default(),
        annotations: Annotations::default(),
        label: None,
        label_type: None,
        kind: StmtKind::Wait(WaitStmt {
            amount: WaitAmount::Seconds(interval),
            until_status: None,
            initial_status: None,
        }),
        transitions: TransitionClause::default(),
        compensation: None,
        comments: CommentSet::default(),
    };
    Ok(WhileStmt {
        cond: cond.ok_or_else(|| WdlError::lower("wait until missing condition"))?,
        negate: true,
        limit: None,
        body: vec![wait],
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
        mode: SwitchMode::Cases,
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
    Ok(MatchArm {
        equals,
        when,
        weight: None,
        toggle: None,
        body,
    })
}

// `toggle <expr> { on -> … off -> … }` -> a two-arm match in Toggle mode. arm order is normalized so
// the `on` arm is always first, keeping lowering and formatting order-independent.
fn parse_toggle(pair: Pair<Rule>) -> Result<MatchStmt, WdlError> {
    let mut subject = Expr::new(ExprKind::Null, Span::default());
    let mut on = None;
    let mut off = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => subject = parse_expr(inner)?,
            Rule::toggle_on => on = Some(parse_arm_body(first_inner(inner)?)?),
            Rule::toggle_off => off = Some(parse_arm_body(first_inner(inner)?)?),
            _ => {}
        }
    }
    let on = on.ok_or_else(|| WdlError::lower("toggle requires an `on` arm"))?;
    let off = off.ok_or_else(|| WdlError::lower("toggle requires an `off` arm"))?;
    let arm = |toggle: bool, body: Block| MatchArm {
        equals: None,
        when: None,
        weight: None,
        toggle: Some(toggle),
        body,
    };
    Ok(MatchStmt {
        subject,
        mode: SwitchMode::Toggle,
        arms: vec![arm(true, on), arm(false, off)],
        default: None,
    })
}

// `split on <expr> { N% -> … … else -> … }` -> a weighted match in Percentage mode.
fn parse_split(pair: Pair<Rule>) -> Result<MatchStmt, WdlError> {
    let mut subject = Expr::new(ExprKind::Null, Span::default());
    let mut arms = Vec::new();
    let mut default = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expr => subject = parse_expr(inner)?,
            Rule::split_arm => arms.push(parse_split_arm(inner)?),
            Rule::match_default => default = Some(parse_arm_body(first_inner(inner)?)?),
            _ => {}
        }
    }
    Ok(MatchStmt {
        subject,
        mode: SwitchMode::Percentage,
        arms,
        default,
    })
}

fn parse_split_arm(pair: Pair<Rule>) -> Result<MatchArm, WdlError> {
    let mut weight = None;
    let mut body = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::integer => weight = Some(parse_i64(inner.as_str(), span_of(&inner))?),
            Rule::arm_body => body = parse_arm_body(inner)?,
            _ => {}
        }
    }
    let weight = weight.ok_or_else(|| WdlError::lower("split arm requires a percentage weight"))?;
    Ok(MatchArm {
        equals: None,
        when: None,
        weight: Some(weight),
        toggle: None,
        body,
    })
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
            // an `edges { … }` block is just a delimited group of outcome and predicate arrows.
            Rule::edges_block => {
                for arrow in inner.into_inner() {
                    match arrow.as_rule() {
                        Rule::outcome_arrow => apply_outcome_arrow(&mut clause, arrow)?,
                        Rule::predicate_arrow => apply_predicate_arrow(&mut clause, arrow)?,
                        _ => {}
                    }
                }
            }
            Rule::outcome_arrow => apply_outcome_arrow(&mut clause, inner)?,
            Rule::predicate_arrow => apply_predicate_arrow(&mut clause, inner)?,
            _ => {}
        }
    }
    Ok(clause)
}

fn apply_outcome_arrow(clause: &mut TransitionClause, pair: Pair<Rule>) -> Result<(), WdlError> {
    let arrow_span = span_of(&pair);
    let mut outcome = String::new();
    let mut target = None;
    for part in pair.into_inner() {
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
    Ok(())
}

fn apply_predicate_arrow(clause: &mut TransitionClause, pair: Pair<Rule>) -> Result<(), WdlError> {
    let arrow_span = span_of(&pair);
    let mut when = None;
    let mut priority = None;
    let mut target = None;
    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::cond => when = Some(parse_cond(part)?),
            Rule::edge_priority => {
                let int = first_inner(part)?;
                priority = Some(parse_i64(int.as_str(), span_of(&int))?);
            }
            Rule::target => target = Some(parse_target(part)?),
            _ => {}
        }
    }
    let when =
        when.ok_or_else(|| WdlError::syntax(arrow_span, "predicate edge missing condition"))?;
    let target =
        target.ok_or_else(|| WdlError::syntax(arrow_span, "predicate edge missing target"))?;
    clause.branches.push(PredicateEdge {
        when,
        target,
        priority,
    });
    Ok(())
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
            CondKind::Exists(parse_condition_expr(
                parse_expr(first_inner(inner)?)?,
                span,
            )?),
            span,
        )),
        Rule::cond_cmp => parse_cond_cmp(inner),
        Rule::expr => Ok(Cond::new(
            CondKind::Expr(parse_condition_expr(parse_expr(inner)?, span)?),
            span,
        )),
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
            // cond_cmp operands are `coalesce_expr` (one level below `expr`) so an expression-level
            // comparison never shadows a condition comparison.
            Rule::coalesce_expr => {
                let operand = parse_condition_expr(parse_coalesce(inner)?, span)?;
                if left.is_none() {
                    left = Some(operand);
                } else {
                    right = Some(operand);
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
    // expr -> lambda | ternary_expr
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::lambda => parse_lambda(inner),
        _ => parse_ternary(inner),
    }
}

fn parse_ternary(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    // ternary_expr -> cast_expr ("?" expr ":" expr)?
    let span = span_of(&pair);
    let mut cond = None;
    let mut then = None;
    let mut els = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::cast_expr => cond = Some(parse_cast(inner)?),
            Rule::expr if then.is_none() => then = Some(parse_expr(inner)?),
            Rule::expr => els = Some(parse_expr(inner)?),
            _ => {}
        }
    }
    let cond = cond.ok_or_else(|| WdlError::lower("empty ternary"))?;
    match (then, els) {
        (Some(then), Some(els)) => Ok(Expr::new(
            ExprKind::Ternary {
                cond: Box::new(cond),
                then: Box::new(then),
                els: Box::new(els),
            },
            span,
        )),
        _ => Ok(cond),
    }
}

fn parse_cast(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    // cast_expr -> compare_expr ("as" type_expr)?
    let span = span_of(&pair);
    let mut expr = None;
    let mut ty = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::compare_expr => expr = Some(parse_compare(inner)?),
            Rule::type_expr => ty = Some(parse_type_expr(inner)?),
            _ => {}
        }
    }
    let expr = expr.ok_or_else(|| WdlError::lower("empty cast"))?;
    match ty {
        Some(ty) => Ok(Expr::new(
            ExprKind::Cast {
                expr: Box::new(expr),
                ty,
            },
            span,
        )),
        None => Ok(expr),
    }
}

fn parse_compare(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    // compare_expr -> coalesce_expr (compare_op coalesce_expr)?
    let span = span_of(&pair);
    let mut left = None;
    let mut op = None;
    let mut right = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::coalesce_expr if left.is_none() => left = Some(parse_coalesce(inner)?),
            Rule::coalesce_expr => right = Some(parse_coalesce(inner)?),
            Rule::compare_op => op = Some(compare_op(inner.as_str())),
            _ => {}
        }
    }
    let left = left.ok_or_else(|| WdlError::lower("empty comparison"))?;
    match (op, right) {
        (Some(op), Some(right)) => Ok(Expr::new(
            ExprKind::Compare {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        )),
        _ => Ok(left),
    }
}

fn compare_op(token: &str) -> CompareOp {
    match token {
        "==" => CompareOp::Eq,
        "!=" => CompareOp::Ne,
        "<=" => CompareOp::Lte,
        ">=" => CompareOp::Gte,
        "<" => CompareOp::Lt,
        _ => CompareOp::Gt,
    }
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
        .filter(|p| p.as_rule() == Rule::sum_expr)
        .map(parse_sum)
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() == 1 {
        return Ok(parts.remove(0));
    }
    Ok(Expr::new(ExprKind::Concat(parts), span))
}

fn parse_sum(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut acc = None;
    let mut op = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::term_expr => {
                let term = parse_term(inner)?;
                acc = Some(match (acc.take(), op.take()) {
                    (None, _) => term,
                    (Some(left), Some("+")) => Expr::new(ExprKind::Add(vec![left, term]), span),
                    (Some(left), _) => Expr::new(ExprKind::Sub(vec![left, term]), span),
                });
            }
            Rule::add_op => op = Some(if inner.as_str() == "+" { "+" } else { "-" }),
            _ => {}
        }
    }
    acc.ok_or_else(|| WdlError::lower("empty arithmetic sum"))
}

fn parse_term(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut acc = None;
    let mut op = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::unary_expr => {
                let factor = parse_unary(inner)?;
                acc = Some(match (acc.take(), op.take()) {
                    (None, _) => factor,
                    (Some(left), Some("*")) => Expr::new(ExprKind::Mul(vec![left, factor]), span),
                    (Some(left), Some("/")) => Expr::new(ExprKind::Div(vec![left, factor]), span),
                    (Some(left), _) => Expr::new(ExprKind::Mod(vec![left, factor]), span),
                });
            }
            Rule::mul_op => {
                op = Some(match inner.as_str() {
                    "*" => "*",
                    "/" => "/",
                    _ => "%",
                })
            }
            _ => {}
        }
    }
    acc.ok_or_else(|| WdlError::lower("empty arithmetic term"))
}

fn parse_unary(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let inner = first_inner(pair)?;
    match inner.as_rule() {
        Rule::neg_expr => {
            let span = span_of(&inner);
            let operand = parse_unary(first_inner(inner)?)?;
            Ok(Expr::new(ExprKind::Neg(Box::new(operand)), span))
        }
        Rule::postfix_expr => parse_postfix(inner),
        other => Err(WdlError::lower(format!("unexpected unary {other:?}"))),
    }
}

// a primary plus a chain of accesses: `.method(args)` (ufcs call), `.key` / `[expr]` (field/index).
fn parse_postfix(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let mut inner = pair.into_inner();
    let primary = inner
        .next()
        .ok_or_else(|| WdlError::lower("postfix primary"))?;
    let mut expr = parse_primary(primary)?;
    for access in inner.filter(|p| p.as_rule() == Rule::access) {
        expr = apply_access(expr, access)?;
    }
    Ok(expr)
}

// apply one access to the running expression. the access form is told apart by its first child:
// an `ident` is a method call, a `path_seg` is a `.key`, and an `expr` is an `[index]`.
fn apply_access(base: Expr, access: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&access);
    let mut parts = access.into_inner();
    let first = parts
        .next()
        .ok_or_else(|| WdlError::lower("empty access"))?;
    match first.as_rule() {
        // `recv.method(args)` desugars to `method(recv, args...)` — the receiver is the first arg.
        Rule::ident => {
            let name = first.as_str().to_string();
            let mut args = vec![base];
            let mut named = Vec::new();
            for arg in parts.filter(|p| p.as_rule() == Rule::call_arg) {
                parse_call_arg(arg, &mut args, &mut named)?;
            }
            // a fluent `recv.method(...)` call: exempt from std-qualification (the receiver carries
            // any namespace), distinguished from a prefix call by `method: true`.
            Ok(Expr::new(
                ExprKind::Call {
                    name,
                    args,
                    named,
                    method: true,
                },
                span,
            ))
        }
        Rule::path_seg => Ok(index_access(base, path_seg_key(first)?, span)),
        Rule::expr => Ok(index_access(base, parse_expr(first)?, span)),
        // `(args)` applies the running value as a first-class closure. arguments are positional; a
        // named argument is meaningless (a lambda's parameters are positional) and is rejected.
        Rule::apply_access => {
            let mut args = Vec::new();
            let mut named = Vec::new();
            for arg in first.into_inner().filter(|p| p.as_rule() == Rule::call_arg) {
                parse_call_arg(arg, &mut args, &mut named)?;
            }
            if !named.is_empty() {
                return Err(WdlError::semantic(
                    span,
                    "applied functions take positional arguments only",
                ));
            }
            Ok(Expr::new(
                ExprKind::Apply {
                    callee: Box::new(base),
                    args,
                },
                span,
            ))
        }
        other => Err(WdlError::lower(format!("unexpected access {other:?}"))),
    }
}

// the key of a `.key` field access: an identifier is a string key, an integer is an index.
fn path_seg_key(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let seg = first_inner(pair)?;
    match seg.as_rule() {
        Rule::ident => Ok(Expr::new(
            ExprKind::Str(vec![StrPart::Lit(seg.as_str().to_string())]),
            span,
        )),
        Rule::integer => Ok(Expr::new(
            ExprKind::Int(parse_i64(seg.as_str(), span_of(&seg))?),
            span,
        )),
        other => Err(WdlError::lower(format!("unexpected access seg {other:?}"))),
    }
}

// fold a static key into a path base (so `foo.bar[0]` stays one `$ref`); otherwise index the
// running value with the `at` intrinsic, which mirrors `$ref` path access (missing -> null).
fn index_access(base: Expr, key: Expr, span: Span) -> Expr {
    if let ExprKind::Path(segs) = &base.kind
        && let Some(seg) = static_path_seg(&key)
    {
        let mut segs = segs.clone();
        segs.push(seg);
        return Expr::new(ExprKind::Path(segs), span);
    }
    Expr::new(
        ExprKind::Call {
            name: "at".to_string(),
            args: vec![base, key],
            named: Vec::new(),
            method: true,
        },
        span,
    )
}

// a key that can extend a path: a non-negative integer index or a non-interpolated string literal.
fn static_path_seg(key: &Expr) -> Option<PathSeg> {
    match &key.kind {
        ExprKind::Int(index) if *index >= 0 => Some(PathSeg::Index(*index as usize)),
        ExprKind::Str(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    StrPart::Lit(lit) => text.push_str(lit),
                    StrPart::Expr(_) => return None,
                }
            }
            Some(PathSeg::Key(text))
        }
        _ => None,
    }
}

fn parse_primary(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let inner = first_inner(pair)?;
    let span = span_of(&inner);
    let kind = match inner.as_rule() {
        Rule::paren_expr => return parse_expr(first_inner(inner)?),
        Rule::file_call => return parse_file_call(inner),
        Rule::dir_call => return parse_dir_call(inner),
        Rule::inline_call => return parse_inline_call(inner),
        Rule::func_call => return parse_func_call(inner),
        Rule::lib_call => return parse_lib_call(inner),
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

fn parse_file_call(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let path = plain_string(first_inner(pair)?)?;
    Ok(Expr::new(ExprKind::FileInclude { path }, span))
}

fn parse_dir_call(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut path = None;
    let mut recursive = false;
    let mut max_depth = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => path = Some(plain_string(inner)?),
            Rule::boolean => recursive = inner.as_str() == "true",
            Rule::number => {
                let depth = parse_i64(inner.as_str(), span)?;
                if depth < 1 {
                    return Err(WdlError::syntax(span, "dir() depth must be at least 1"));
                }
                max_depth = Some(depth as usize);
            }
            _ => {}
        }
    }
    let path = path.ok_or_else(|| WdlError::syntax(span, "dir() missing path"))?;
    Ok(Expr::new(
        ExprKind::DirInclude {
            path,
            recursive,
            max_depth,
        },
        span,
    ))
}

fn parse_inline_call(pair: Pair<Rule>) -> Result<Expr, WdlError> {
    let span = span_of(&pair);
    let mut language = None;
    let mut content = None;
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => language = Some(plain_string(inner)?),
            Rule::raw_block => content = Some(raw_block_content(inner.as_str())),
            _ => {}
        }
    }
    Ok(Expr::new(
        ExprKind::InlineCode {
            language: language.unwrap_or_default(),
            content: content.unwrap_or_default(),
        },
        span,
    ))
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
            Rule::object_spread => entries.push(parse_spread_entry(inner)?),
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

fn raw_block_content(text: &str) -> String {
    let Some(content) = text
        .strip_prefix("```")
        .and_then(|text| text.strip_suffix("```"))
    else {
        return text.to_string();
    };
    if let Some(stripped) = content.strip_prefix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = content.strip_prefix('\n') {
        stripped.to_string()
    } else {
        content.to_string()
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

fn expect_bool(value: &Expr, label: &str) -> Result<bool, WdlError> {
    match &value.kind {
        ExprKind::Bool(flag) => Ok(*flag),
        _ => Err(WdlError::lower(format!("{label} expects a boolean"))),
    }
}

/// accepts either a bare identifier (`failure`) or a literal string (`"failure"`).
fn expect_ident_or_string(value: &Expr, label: &str) -> Result<String, WdlError> {
    match &value.kind {
        ExprKind::Path(segs) if segs.len() == 1 => match &segs[0] {
            crate::ast::PathSeg::Key(key) => Ok(key.clone()),
            crate::ast::PathSeg::Index(_) => expect_string(value, label),
        },
        _ => expect_string(value, label),
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
