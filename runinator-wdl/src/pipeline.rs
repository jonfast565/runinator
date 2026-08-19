// the `.wdlp` pipeline surface: `pipeline "Name" { workflow "…" … "A" -> "B" on <selector> }`.
// lowers to a portable `PipelineBundle` (members and links by workflow name) for import; the web
// service resolves names to ids and persists the graph atomically. the reverse
// (`pipeline_to_wdlp`) re-renders a bundle so exports round-trip and the editor can format.

use std::collections::{BTreeMap, HashMap, HashSet};

use runinator_models::pipelines::{
    PipelineBundle, PipelineDefaults, PipelineFailurePolicy, PipelineJoinMode, PipelineJoinSpec,
    PipelineLinkSelector, PipelineLinkSpec, PipelineMemberFailureMode, PipelineMemberSpec,
    PipelineSpec, PipelineTriggerSpec,
};
use runinator_models::schedules::{ConcurrencyPolicy, WorkflowConcurrency};
use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowTriggerKind;

use crate::ast::{PipelineDecl, PipelineLinkDecl, PipelineMemberDecl, PipelineTriggerDecl};
use crate::errors::WdlError;
use runinator_wdl_syntax::parser::parse_pipeline_document;

/// parse `.wdlp` source into a `PipelineBundle`. rejects empty names, empty member lists, and links
/// whose endpoints are not declared members so a compiled pipeline is always well-formed.
pub fn parse_pipeline_str(src: &str) -> Result<PipelineBundle, WdlError> {
    let decls = parse_pipeline_document(src)?;
    let mut pipelines = Vec::with_capacity(decls.len());
    for decl in &decls {
        pipelines.push(lower_pipeline(decl)?);
    }
    Ok(PipelineBundle { pipelines })
}

fn lower_pipeline(decl: &PipelineDecl) -> Result<PipelineSpec, WdlError> {
    if decl.name.trim().is_empty() {
        return Err(WdlError::syntax(
            decl.span,
            "pipeline name must not be empty",
        ));
    }
    if decl.members.is_empty() {
        return Err(WdlError::syntax(
            decl.span,
            "a pipeline must declare at least one `workflow` member",
        ));
    }
    let on_step_failure = match decl.on_failure.as_deref() {
        Some("continue") => PipelineFailurePolicy::Continue,
        // absent or "halt" -> halt (the default).
        _ => PipelineFailurePolicy::Halt,
    };
    let defaults = PipelineDefaults {
        on_step_failure,
        max_chain_depth: decl.max_depth,
        ..PipelineDefaults::default()
    };
    let members: HashSet<&str> = decl.members.iter().map(|m| m.name.as_str()).collect();
    let mut links = Vec::with_capacity(decl.links.len());
    for link in &decl.links {
        links.push(lower_link(link, &members, on_step_failure)?);
    }
    validate_links(&links, &members, decl.span)?;
    let mut joins = Vec::with_capacity(decl.joins.len());
    for join in &decl.joins {
        if !members.contains(join.target.as_str()) {
            return Err(WdlError::syntax(
                join.span,
                format!(
                    "join target \"{}\" is not a declared workflow member",
                    join.target
                ),
            ));
        }
        joins.push(PipelineJoinSpec {
            target: join.target.clone(),
            mode: match join.mode.as_str() {
                "any" => PipelineJoinMode::Any,
                "first_success" => PipelineJoinMode::FirstSuccess,
                _ => PipelineJoinMode::All,
            },
            parameters: lower_mapping(join.parameters.as_ref())?,
        });
    }
    validate_joins(&links, &joins, decl.span)?;
    let mut triggers = Vec::with_capacity(decl.triggers.len());
    for trigger in &decl.triggers {
        triggers.push(lower_trigger(trigger)?);
    }
    let members = decl
        .members
        .iter()
        .map(lower_member)
        .collect::<Result<Vec<_>, WdlError>>()?;
    Ok(PipelineSpec {
        name: decl.name.clone(),
        description: decl.description.clone(),
        defaults,
        members,
        links,
        joins,
        concurrency: decl
            .concurrency
            .as_ref()
            .map(|c| WorkflowConcurrency {
                max_concurrent_runs: c.max_concurrent_runs,
                on_conflict: match c.on_conflict {
                    crate::ast::ConcurrencyPolicy::Allow => ConcurrencyPolicy::Allow,
                    crate::ast::ConcurrencyPolicy::Queue => ConcurrencyPolicy::Queue,
                    crate::ast::ConcurrencyPolicy::CancelPrevious => {
                        ConcurrencyPolicy::CancelPrevious
                    }
                    crate::ast::ConcurrencyPolicy::Skip => ConcurrencyPolicy::Skip,
                },
            })
            .unwrap_or_default(),
        triggers,
    })
}

/// lower a `workflow "Name" [on_failure <mode>]` member decl. `on_failure` is `None` when the member
/// declares no override, meaning it takes the pipeline's `default_failure_mode` at import.
fn lower_member(decl: &PipelineMemberDecl) -> Result<PipelineMemberSpec, WdlError> {
    let failure_mode = match decl.on_failure.as_deref() {
        None => None,
        Some("stop") => Some(PipelineMemberFailureMode::Stop),
        Some("continue") => Some(PipelineMemberFailureMode::Continue),
        Some("silently_continue") => Some(PipelineMemberFailureMode::SilentlyContinue),
        Some("inquire") => Some(PipelineMemberFailureMode::Inquire),
        Some(other) => {
            return Err(WdlError::syntax(
                decl.span,
                format!("unknown member failure mode \"{other}\""),
            ));
        }
    };
    Ok(PipelineMemberSpec {
        name: decl.name.clone(),
        failure_mode,
    })
}

/// lower a parsed pipeline trigger decl into a portable `PipelineTriggerSpec`. a cron trigger carries
/// `{cron, parameters}`; a chained trigger carries `{on, source_workflow | source_pipeline}`.
fn lower_trigger(decl: &PipelineTriggerDecl) -> Result<PipelineTriggerSpec, WdlError> {
    if let Some(cron) = &decl.cron {
        return Ok(PipelineTriggerSpec {
            kind: WorkflowTriggerKind::Cron,
            enabled: !decl.disabled,
            configuration: runinator_models::json!({ "cron": cron, "parameters": {} }),
        });
    }
    let source = decl.source.clone().ok_or_else(|| {
        WdlError::syntax(decl.span, "a chained pipeline trigger needs a source name")
    })?;
    // map the raw chain event keyword to the `on` selector.
    let on = match decl.event.as_deref() {
        Some("on_failure") => "failure",
        Some("on_complete") => "complete",
        _ => "success",
    };
    let source_field = match decl.source_kind.as_deref() {
        Some("pipeline") => "source_pipeline",
        _ => "source_workflow",
    };
    Ok(PipelineTriggerSpec {
        kind: WorkflowTriggerKind::Chained,
        enabled: !decl.disabled,
        configuration: runinator_models::json!({
            "on": on,
            source_field: source,
            "parameters": {},
        }),
    })
}

fn lower_link(
    link: &PipelineLinkDecl,
    members: &HashSet<&str>,
    policy: PipelineFailurePolicy,
) -> Result<PipelineLinkSpec, WdlError> {
    if !members.contains(link.from.as_str()) {
        return Err(WdlError::syntax(
            link.span,
            format!(
                "link source \"{}\" is not a declared workflow member",
                link.from
            ),
        ));
    }
    if !members.contains(link.to.as_str()) {
        return Err(WdlError::syntax(
            link.span,
            format!(
                "link target \"{}\" is not a declared workflow member",
                link.to
            ),
        ));
    }
    // an explicit `on <selector>` wins; otherwise the failure policy seeds it (halt -> success,
    // continue -> complete), matching the pipeline authoring defaults.
    let on = match link.on.as_deref() {
        Some("complete") => PipelineLinkSelector::Complete,
        Some("failure") => PipelineLinkSelector::Failure,
        Some("success") => PipelineLinkSelector::Success,
        _ => match policy {
            PipelineFailurePolicy::Continue => PipelineLinkSelector::Complete,
            PipelineFailurePolicy::Halt => PipelineLinkSelector::Success,
        },
    };
    Ok(PipelineLinkSpec {
        from: link.from.clone(),
        to: link.to.clone(),
        on,
        enabled: true,
        parameters: lower_mapping(link.parameters.as_ref())?,
    })
}

fn lower_mapping(expr: Option<&crate::ast::Expr>) -> Result<Value, WdlError> {
    match expr {
        Some(expr) => {
            let value = runinator_wdl_codegen::lower::lower_expression_fragment(
                expr,
                &crate::CompileOptions::default(),
            )?;
            validate_mapping_roots(&value, expr.span)?;
            Ok(value)
        }
        None => Ok(Value::Object(Map::new())),
    }
}

fn validate_mapping_roots(value: &Value, span: crate::errors::Span) -> Result<(), WdlError> {
    match value {
        Value::Array(values) => {
            for value in values {
                validate_mapping_roots(value, span)?;
            }
        }
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_object) {
                if let Some(node) = reference.get("node").and_then(Value::as_str)
                    && !matches!(node, "source" | "members")
                {
                    return Err(WdlError::syntax(
                        span,
                        format!(
                            "pipeline mapping root '{node}' is unsupported; use params, source, or members"
                        ),
                    ));
                }
            }
            if let Some(call) = object.get("$call").and_then(Value::as_str) {
                let leaf = call.rsplit('.').next().unwrap_or(call);
                if !runinator_workflows::PureIntrinsics::contains(leaf)
                    && !runinator_workflows::is_higher_order(leaf)
                {
                    return Err(WdlError::syntax(
                        span,
                        format!("pipeline mapping call '{call}' must be a pure intrinsic"),
                    ));
                }
            }
            for value in object.values() {
                validate_mapping_roots(value, span)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_links(
    links: &[PipelineLinkSpec],
    members: &HashSet<&str>,
    span: crate::errors::Span,
) -> Result<(), WdlError> {
    let mut seen = HashSet::new();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for link in links {
        if link.from == link.to {
            return Err(WdlError::syntax(
                span,
                "pipeline links cannot target themselves",
            ));
        }
        if !seen.insert((link.from.as_str(), link.to.as_str())) {
            return Err(WdlError::syntax(
                span,
                format!("duplicate pipeline link {} -> {}", link.from, link.to),
            ));
        }
        adjacency.entry(&link.from).or_default().push(&link.to);
    }
    fn visit<'a>(
        node: &'a str,
        adjacency: &HashMap<&'a str, Vec<&'a str>>,
        visiting: &mut HashSet<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> bool {
        if done.contains(node) {
            return false;
        }
        if !visiting.insert(node) {
            return true;
        }
        if adjacency
            .get(node)
            .is_some_and(|next| next.iter().any(|n| visit(n, adjacency, visiting, done)))
        {
            return true;
        }
        visiting.remove(node);
        done.insert(node);
        false
    }
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    if members
        .iter()
        .any(|m| visit(m, &adjacency, &mut visiting, &mut done))
    {
        return Err(WdlError::syntax(span, "pipeline graph must be acyclic"));
    }
    Ok(())
}

fn validate_joins(
    links: &[PipelineLinkSpec],
    joins: &[PipelineJoinSpec],
    span: crate::errors::Span,
) -> Result<(), WdlError> {
    let mut inbound: HashMap<&str, Vec<&PipelineLinkSpec>> = HashMap::new();
    for link in links.iter().filter(|l| l.enabled) {
        inbound.entry(&link.to).or_default().push(link);
    }
    let join_by_target: BTreeMap<&str, &PipelineJoinSpec> =
        joins.iter().map(|j| (j.target.as_str(), j)).collect();
    for (target, incoming) in inbound {
        if incoming.len() > 1 && !join_by_target.contains_key(target) {
            return Err(WdlError::syntax(
                span,
                format!(
                    "member \"{target}\" has multiple inbound links and requires an explicit join"
                ),
            ));
        }
    }
    for join in joins {
        let incoming = links
            .iter()
            .filter(|l| l.enabled && l.to == join.target)
            .collect::<Vec<_>>();
        if incoming.len() < 2 {
            return Err(WdlError::syntax(
                span,
                format!(
                    "join target \"{}\" needs at least two inbound links",
                    join.target
                ),
            ));
        }
        if join.mode == PipelineJoinMode::FirstSuccess
            && incoming
                .iter()
                .any(|l| l.on != PipelineLinkSelector::Success)
        {
            return Err(WdlError::syntax(
                span,
                "first_success joins require success-selecting inbound links",
            ));
        }
    }
    Ok(())
}

/// render a `PipelineBundle` back into `.wdlp` source so exports round-trip and the editor can
/// format a pipeline file.
pub fn pipeline_to_wdlp(bundle: &PipelineBundle) -> String {
    let mut out = String::new();
    for (index, spec) in bundle.pipelines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!("pipeline {} {{\n", quote(&spec.name)));
        if let Some(description) = &spec.description {
            out.push_str(&format!("    description {}\n", quote(description)));
        }
        if spec.defaults.on_step_failure == PipelineFailurePolicy::Continue {
            out.push_str("    on_failure continue\n");
        }
        if let Some(max_depth) = spec.defaults.max_chain_depth {
            out.push_str(&format!("    max_depth {max_depth}\n"));
        }
        if spec.concurrency.max_concurrent_runs > 0 {
            out.push_str(&format!(
                "    concurrency {} on_conflict {}\n",
                spec.concurrency.max_concurrent_runs,
                spec.concurrency.on_conflict.as_str()
            ));
        }
        if !spec.triggers.is_empty() {
            out.push('\n');
            for trigger in &spec.triggers {
                out.push_str(&render_trigger(trigger));
            }
        }
        if !spec.members.is_empty() {
            out.push('\n');
            for member in &spec.members {
                match member.failure_mode {
                    Some(mode) => out.push_str(&format!(
                        "    workflow {} on_failure {}\n",
                        quote(&member.name),
                        mode.as_str()
                    )),
                    None => out.push_str(&format!("    workflow {}\n", quote(&member.name))),
                }
            }
        }
        if !spec.links.is_empty() {
            out.push('\n');
            for link in &spec.links {
                let mapping = if link.parameters.as_object().is_some_and(|m| !m.is_empty()) {
                    format!(
                        " with {}",
                        runinator_wdl_codegen::render_expression(&link.parameters)
                            .unwrap_or_else(|_| "{}".into())
                    )
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "    {} -> {} on {}{}\n",
                    quote(&link.from),
                    quote(&link.to),
                    link.on.as_str(),
                    mapping,
                ));
            }
        }
        if !spec.joins.is_empty() {
            for join in &spec.joins {
                let mode = match join.mode {
                    PipelineJoinMode::All => "all",
                    PipelineJoinMode::Any => "any",
                    PipelineJoinMode::FirstSuccess => "first_success",
                };
                let mapping = if join.parameters.as_object().is_some_and(|m| !m.is_empty()) {
                    format!(
                        " with {}",
                        runinator_wdl_codegen::render_expression(&join.parameters)
                            .unwrap_or_else(|_| "{}".into())
                    )
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "    join {} {mode}{mapping}\n",
                    quote(&join.target)
                ));
            }
        }
        out.push_str("}\n");
    }
    out
}

/// render a pipeline trigger spec back to `.wdlp` source. mirrors `lower_trigger` so files round-trip.
fn render_trigger(trigger: &PipelineTriggerSpec) -> String {
    let config = &trigger.configuration;
    let disabled = if trigger.enabled { "" } else { " disabled" };
    if trigger.kind == WorkflowTriggerKind::Cron {
        let cron = config.get("cron").and_then(|v| v.as_str()).unwrap_or("");
        return format!("    trigger cron {}{}\n", quote(cron), disabled);
    }
    let on = config
        .get("on")
        .and_then(|v| v.as_str())
        .unwrap_or("success");
    let event = match on {
        "failure" => "on_failure",
        "complete" => "on_complete",
        _ => "on_success",
    };
    let (source_kind, source) =
        if let Some(name) = config.get("source_pipeline").and_then(|v| v.as_str()) {
            ("pipeline", name)
        } else {
            (
                "workflow",
                config
                    .get("source_workflow")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )
        };
    format!(
        "    trigger {} {} {}{}\n",
        event,
        source_kind,
        quote(source),
        disabled
    )
}

fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
