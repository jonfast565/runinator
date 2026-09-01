// the `.rexrapp` pipeline surface: `pipeline "Name" { workflow "namespace.key" … }`.
// lowers to a portable `PipelineBundle` (members and links by canonical workflow path) for import;
// the web service resolves paths to ids and persists the graph atomically. the reverse
// (`pipeline_to_rexrapp`) re-renders a bundle so exports round-trip and the editor can format.

use std::collections::{BTreeMap, HashMap, HashSet};

use runinator_models::orchestration::{
    BudgetExhaustion, BudgetPolicy, ControlEffect, EpochStopAction, IngressAction,
    IngressLifecycle, IngressPolicy, IngressPredicate, IngressPredicateOperator, IngressRoute,
    IntentPolicy, OrchestrationPolicy, PhasePolicy, RestartSelector, WorkspacePolicy,
    WorkspaceRecovery,
};
use runinator_models::pipelines::{
    PipelineBundle, PipelineDefaults, PipelineFailurePolicy, PipelineJoinMode, PipelineJoinSpec,
    PipelineLinkSelector, PipelineLinkSpec, PipelineMemberFailureMode, PipelineMemberSpec,
    PipelineSpec, PipelineTriggerSpec,
};
use runinator_models::schedules::{ConcurrencyPolicy, WorkflowConcurrency};
use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowTriggerKind;

use crate::ast::{PipelineDecl, PipelineLinkDecl, PipelineMemberDecl, PipelineTriggerDecl};
use crate::errors::RexRapError;
use runinator_rexrap_syntax::parser::parse_pipeline_document;

/// parse `.rexrapp` source into a `PipelineBundle`. rejects empty names, empty member lists, and links
/// whose endpoints are not declared members so a compiled pipeline is always well-formed.
pub fn parse_pipeline_str(src: &str) -> Result<PipelineBundle, RexRapError> {
    let decls = parse_pipeline_document(src)?;
    let mut pipelines = Vec::with_capacity(decls.len());
    for decl in &decls {
        pipelines.push(lower_pipeline(decl)?);
    }
    Ok(PipelineBundle { pipelines })
}

fn lower_pipeline(decl: &PipelineDecl) -> Result<PipelineSpec, RexRapError> {
    if decl.name.trim().is_empty() {
        return Err(RexRapError::syntax(
            decl.span,
            "pipeline name must not be empty",
        ));
    }
    if decl.members.is_empty() {
        return Err(RexRapError::syntax(
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
    let member_names: HashSet<&str> = decl.members.iter().map(|m| m.name.as_str()).collect();
    let mut links = Vec::with_capacity(decl.links.len());
    for link in &decl.links {
        links.push(lower_link(link, &member_names, on_step_failure)?);
    }
    validate_links(&links, &member_names, decl.span)?;
    let mut joins = Vec::with_capacity(decl.joins.len());
    for join in &decl.joins {
        if !member_names.contains(join.target.as_str()) {
            return Err(RexRapError::syntax(
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
        .collect::<Result<Vec<_>, RexRapError>>()?;
    let mut metadata = lower_ingress_metadata(decl.ingress.as_ref())?;
    let orchestration = decl
        .orchestration
        .as_ref()
        .map(|orchestration| lower_orchestration(orchestration, &member_names))
        .transpose()?;
    if let Some(policy) = orchestration.as_ref() {
        metadata
            .as_object_mut()
            .expect("pipeline metadata is an object")
            .insert(
                "orchestration".into(),
                serde_json::to_value(policy)
                    .expect("orchestration policy serializes")
                    .into(),
            );
    }
    if orchestration.is_some() && decl.ingress.is_none() {
        return Err(RexRapError::syntax(
            decl.span,
            "a pipeline orchestration policy requires an ingress policy",
        ));
    }
    if let Some(ingress) = metadata.get("ingress") {
        let ingress: IngressPolicy = serde_json::from_value(ingress.clone().into())
            .expect("lowered ingress policy deserializes");
        ingress
            .validate_dispatches(orchestration.as_ref())
            .map_err(|message| RexRapError::syntax(decl.span, message))?;
    }
    Ok(PipelineSpec {
        name: decl.name.clone(),
        key: decl.key.clone(),
        namespace: decl.namespace.clone(),
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
        metadata,
        triggers,
    })
}

fn lower_orchestration(
    decl: &crate::ast::OrchestrationDecl,
    members: &HashSet<&str>,
) -> Result<OrchestrationPolicy, RexRapError> {
    let mut policy = OrchestrationPolicy::default();
    for intent in &decl.intents {
        let effect = match intent.effect.as_str() {
            "terminate" => ControlEffect::Terminate,
            "suspend" => ControlEffect::Suspend,
            "resume" => ControlEffect::Resume,
            "supersede" => ControlEffect::Supersede,
            "observe" => ControlEffect::Observe,
            "signal" => ControlEffect::Signal,
            _ => unreachable!("parser restricts orchestration effects"),
        };
        let stop = match intent.stop.as_deref() {
            Some("pause") => EpochStopAction::Pause,
            Some("none") => EpochStopAction::None,
            _ => EpochStopAction::Cancel,
        };
        let restart = match intent.restart.as_deref() {
            None | Some("entry") => RestartSelector::Entry,
            Some("current") => RestartSelector::Current,
            Some(member) => RestartSelector::Member(member.to_string()),
        };
        if policy
            .intents
            .insert(
                intent.name.clone(),
                IntentPolicy {
                    effect,
                    priority: intent.priority,
                    coalesce_seconds: intent.coalesce_seconds,
                    stop,
                    restart,
                    subject_revision_pointer: intent.revision.clone(),
                    allow_self_originated: intent.allow_self_originated,
                    signal_name: intent.signal_name.clone(),
                },
            )
            .is_some()
        {
            return Err(RexRapError::syntax(
                intent.span,
                format!("duplicate orchestration intent '{}'", intent.name),
            ));
        }
    }
    for budget in &decl.budgets {
        let exhausted = match budget.exhausted.as_str() {
            "fail" => BudgetExhaustion::Fail,
            "terminate" => BudgetExhaustion::Terminate,
            _ => BudgetExhaustion::Pause,
        };
        if policy
            .budgets
            .insert(
                budget.name.clone(),
                BudgetPolicy {
                    attempts: budget.attempts,
                    exhausted,
                    handoff: budget.handoff.clone(),
                },
            )
            .is_some()
        {
            return Err(RexRapError::syntax(
                budget.span,
                format!("duplicate orchestration budget '{}'", budget.name),
            ));
        }
    }
    for phase in &decl.phases {
        if !members.contains(phase.member.as_str()) {
            return Err(RexRapError::syntax(
                phase.span,
                format!(
                    "orchestration phase '{}' is not a pipeline member",
                    phase.member
                ),
            ));
        }
        let mut phase_policy = PhasePolicy::default();
        for (field, pointer) in &phase.mappings {
            let slot = match field.as_str() {
                "subject_revision" => &mut phase_policy.result.subject_revision,
                "resources" => &mut phase_policy.result.resources,
                "evidence" => &mut phase_policy.result.evidence,
                "failure_class" => &mut phase_policy.result.failure_class,
                "correlations" => &mut phase_policy.result.correlations,
                _ => unreachable!("parser restricts result mapping fields"),
            };
            if slot.replace(pointer.clone()).is_some() {
                return Err(RexRapError::syntax(
                    phase.span,
                    format!("phase '{}' maps '{field}' more than once", phase.member),
                ));
            }
        }
        if let Some(workspace) = &phase.workspace {
            let requirements = workspace
                .labels
                .as_ref()
                .map(|expr| {
                    runinator_rexrap_codegen::lower::lower_expression_fragment(
                        expr,
                        &crate::CompileOptions::default(),
                    )
                })
                .transpose()?
                .unwrap_or_else(|| Value::Object(Map::new()));
            if contains_dynamic_expression(&requirements) {
                return Err(RexRapError::syntax(
                    workspace.span,
                    "workspace labels must be literal values",
                ));
            }
            phase_policy.workspace = Some(WorkspacePolicy {
                scope: workspace.scope.clone(),
                requirements,
                lease_seconds: workspace.lease_seconds.unwrap_or(300),
                reuse: workspace.reuse,
                recovery: match workspace.recovery.as_deref() {
                    Some("wait") => WorkspaceRecovery::Wait,
                    Some("fail") => WorkspaceRecovery::Fail,
                    _ => WorkspaceRecovery::Replace,
                },
            });
        }
        if policy
            .phases
            .insert(phase.member.clone(), phase_policy)
            .is_some()
        {
            return Err(RexRapError::syntax(
                phase.span,
                format!("duplicate orchestration phase '{}'", phase.member),
            ));
        }
    }
    policy
        .validate(members.iter().copied())
        .map_err(|message| RexRapError::syntax(decl.span, message))?;
    Ok(policy)
}

fn lower_ingress_metadata(ingress: Option<&crate::ast::IngressDecl>) -> Result<Value, RexRapError> {
    let Some(ingress) = ingress else {
        return Ok(Value::Object(Map::new()));
    };
    let mut routes = Vec::with_capacity(ingress.routes.len());
    for route in &ingress.routes {
        let lifecycle = match route.lifecycle.as_str() {
            "unbound" => IngressLifecycle::Unbound,
            "active" => IngressLifecycle::Active,
            "terminal" => IngressLifecycle::Terminal,
            _ => unreachable!("parser restricts ingress lifecycle"),
        };
        let action = match route.action.as_str() {
            "start" => IngressAction::Start,
            "interrupt" => IngressAction::Interrupt,
            "queue" => IngressAction::Queue,
            "record" => IngressAction::Record,
            "requeue" => IngressAction::Requeue,
            "dispatch" => IngressAction::Dispatch,
            _ => unreachable!("parser restricts ingress action"),
        };
        if !action.is_allowed_when(lifecycle) {
            return Err(RexRapError::syntax(
                route.span,
                format!(
                    "ingress action '{}' is not valid when the admission is {}",
                    route.action, route.lifecycle
                ),
            ));
        }
        let predicates = route
            .predicates
            .iter()
            .map(|predicate| {
                let operator = match predicate.operator.as_str() {
                    "==" => IngressPredicateOperator::Equal,
                    "!=" => IngressPredicateOperator::NotEqual,
                    "in" => IngressPredicateOperator::In,
                    "contains" => IngressPredicateOperator::Contains,
                    "exists" => IngressPredicateOperator::Exists,
                    _ => unreachable!("parser restricts ingress predicate operators"),
                };
                let value = predicate
                    .value
                    .as_ref()
                    .map(|expr| {
                        runinator_rexrap_codegen::lower::lower_expression_fragment(
                            expr,
                            &crate::CompileOptions::default(),
                        )
                    })
                    .transpose()?;
                if value.as_ref().is_some_and(contains_dynamic_expression) {
                    return Err(RexRapError::syntax(
                        predicate.span,
                        "ingress predicate values must be literals",
                    ));
                }
                Ok(IngressPredicate {
                    pointer: predicate.pointer.clone(),
                    operator,
                    value,
                })
            })
            .collect::<Result<Vec<_>, RexRapError>>()?;
        routes.push(IngressRoute {
            event_type: route.event_type.clone(),
            lifecycle,
            action,
            predicates,
            intent: route.intent.clone(),
        });
    }
    let policy = IngressPolicy {
        scope: ingress.scope.clone(),
        routes,
    };
    policy
        .validate()
        .map_err(|message| RexRapError::syntax(ingress.span, message))?;
    let mut metadata = Map::new();
    metadata.insert(
        "ingress".into(),
        serde_json::to_value(policy)
            .expect("ingress policy serializes")
            .into(),
    );
    Ok(Value::Object(metadata))
}

fn contains_dynamic_expression(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_dynamic_expression),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.starts_with('$') || contains_dynamic_expression(value)),
        _ => false,
    }
}

/// lower a `workflow "Name" [on_failure <mode>]` member decl. `on_failure` is `None` when the member
/// declares no override, meaning it takes the pipeline's `default_failure_mode` at import.
fn lower_member(decl: &PipelineMemberDecl) -> Result<PipelineMemberSpec, RexRapError> {
    require_canonical_path(&decl.name, decl.span, "pipeline member workflow")?;
    let failure_mode = match decl.on_failure.as_deref() {
        None => None,
        Some("stop") => Some(PipelineMemberFailureMode::Stop),
        Some("continue") => Some(PipelineMemberFailureMode::Continue),
        Some("silently_continue") => Some(PipelineMemberFailureMode::SilentlyContinue),
        Some("inquire") => Some(PipelineMemberFailureMode::Inquire),
        Some(other) => {
            return Err(RexRapError::syntax(
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
fn lower_trigger(decl: &PipelineTriggerDecl) -> Result<PipelineTriggerSpec, RexRapError> {
    if let Some(cron) = &decl.cron {
        return Ok(PipelineTriggerSpec {
            kind: WorkflowTriggerKind::Cron,
            enabled: !decl.disabled,
            configuration: runinator_models::json!({ "cron": cron, "parameters": {} }),
        });
    }
    if let Some(schedule) = &decl.schedule {
        let schedule = lower_mapping(Some(schedule))?;
        if contains_dynamic_expression(&schedule) {
            return Err(RexRapError::syntax(
                decl.span,
                "a pipeline schedule must be a static object literal",
            ));
        }
        let exclusions = decl
            .exclusions
            .iter()
            .map(|value| {
                let value = lower_mapping(Some(value))?;
                if contains_dynamic_expression(&value) {
                    return Err(RexRapError::syntax(
                        decl.span,
                        "a pipeline schedule exclusion must be a static object literal",
                    ));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, RexRapError>>()?;
        return Ok(PipelineTriggerSpec {
            kind: WorkflowTriggerKind::Cron,
            enabled: !decl.disabled,
            configuration: runinator_models::json!({
                "schedule": schedule,
                "exclusions": exclusions,
                "parameters": {},
            }),
        });
    }
    let source = decl.source.clone().ok_or_else(|| {
        RexRapError::syntax(decl.span, "a chained pipeline trigger needs a source name")
    })?;
    require_canonical_path(&source, decl.span, "pipeline trigger source")?;
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

fn require_canonical_path(
    value: &str,
    span: crate::errors::Span,
    kind: &str,
) -> Result<(), RexRapError> {
    let mut segments = value.split('.');
    let valid = segments.clone().count() > 1
        && segments.all(|segment| {
            let mut chars = segment.chars();
            matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
                && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        });
    if valid {
        Ok(())
    } else {
        Err(RexRapError::syntax(
            span,
            format!("{kind} '{value}' must be a canonical namespace.key path"),
        ))
    }
}

fn lower_link(
    link: &PipelineLinkDecl,
    members: &HashSet<&str>,
    policy: PipelineFailurePolicy,
) -> Result<PipelineLinkSpec, RexRapError> {
    if !members.contains(link.from.as_str()) {
        return Err(RexRapError::syntax(
            link.span,
            format!(
                "link source \"{}\" is not a declared workflow member",
                link.from
            ),
        ));
    }
    if !members.contains(link.to.as_str()) {
        return Err(RexRapError::syntax(
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

fn lower_mapping(expr: Option<&crate::ast::Expr>) -> Result<Value, RexRapError> {
    match expr {
        Some(expr) => {
            let value = runinator_rexrap_codegen::lower::lower_expression_fragment(
                expr,
                &crate::CompileOptions::default(),
            )?;
            validate_mapping_roots(&value, expr.span)?;
            Ok(value)
        }
        None => Ok(Value::Object(Map::new())),
    }
}

fn validate_mapping_roots(value: &Value, span: crate::errors::Span) -> Result<(), RexRapError> {
    match value {
        Value::Array(values) => {
            for value in values {
                validate_mapping_roots(value, span)?;
            }
        }
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_object)
                && let Some(node) = reference.get("node").and_then(Value::as_str)
                && !matches!(node, "source" | "members")
            {
                return Err(RexRapError::syntax(
                    span,
                    format!(
                        "pipeline mapping root '{node}' is unsupported; use params, source, or members"
                    ),
                ));
            }
            if let Some(call) = object.get("$call").and_then(Value::as_str) {
                let leaf = call.rsplit('.').next().unwrap_or(call);
                if !runinator_workflows::PureIntrinsics::contains(leaf)
                    && !runinator_workflows::is_higher_order(leaf)
                {
                    return Err(RexRapError::syntax(
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
) -> Result<(), RexRapError> {
    let mut seen = HashSet::new();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for link in links {
        if link.from == link.to {
            return Err(RexRapError::syntax(
                span,
                "pipeline links cannot target themselves",
            ));
        }
        if !seen.insert((link.from.as_str(), link.to.as_str())) {
            return Err(RexRapError::syntax(
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
        return Err(RexRapError::syntax(span, "pipeline graph must be acyclic"));
    }
    Ok(())
}

fn validate_joins(
    links: &[PipelineLinkSpec],
    joins: &[PipelineJoinSpec],
    span: crate::errors::Span,
) -> Result<(), RexRapError> {
    let mut inbound: HashMap<&str, Vec<&PipelineLinkSpec>> = HashMap::new();
    for link in links.iter().filter(|l| l.enabled) {
        inbound.entry(&link.to).or_default().push(link);
    }
    let join_by_target: BTreeMap<&str, &PipelineJoinSpec> =
        joins.iter().map(|j| (j.target.as_str(), j)).collect();
    for (target, incoming) in inbound {
        if incoming.len() > 1 && !join_by_target.contains_key(target) {
            return Err(RexRapError::syntax(
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
            return Err(RexRapError::syntax(
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
            return Err(RexRapError::syntax(
                span,
                "first_success joins require success-selecting inbound links",
            ));
        }
    }
    Ok(())
}

/// render a `PipelineBundle` back into `.rexrapp` source so exports round-trip and the editor can
/// format a pipeline file.
pub fn pipeline_to_rexrapp(bundle: &PipelineBundle) -> String {
    let mut out = String::new();
    for (index, spec) in bundle.pipelines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!("pipeline {} {{\n", quote(&spec.name)));
        if let Some(key) = &spec.key {
            out.push_str(&format!("    key {key}\n"));
        }
        if let Some(namespace) = &spec.namespace {
            out.push_str(&format!("    namespace {namespace}\n"));
        }
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
        if let Some(ingress) = spec
            .metadata
            .get("ingress")
            .and_then(|value| serde_json::from_value::<IngressPolicy>(value.clone().into()).ok())
        {
            out.push('\n');
            out.push_str(&format!("    ingress scope {} {{\n", quote(&ingress.scope)));
            for route in ingress.routes {
                out.push_str(&format!(
                    "        on {} when {}\n",
                    quote(&route.event_type),
                    ingress_lifecycle_name(route.lifecycle)
                ));
                for predicate in route.predicates {
                    let operator = match predicate.operator {
                        IngressPredicateOperator::Equal => "==",
                        IngressPredicateOperator::NotEqual => "!=",
                        IngressPredicateOperator::In => "in",
                        IngressPredicateOperator::Contains => "contains",
                        IngressPredicateOperator::Exists => "exists",
                    };
                    let value = predicate
                        .value
                        .as_ref()
                        .map(|value| {
                            runinator_rexrap_codegen::render_expression(value)
                                .unwrap_or_else(|_| "null".into())
                        })
                        .unwrap_or_default();
                    out.push_str(
                        format!(
                            "            if {} {operator} {value}\n",
                            quote(&predicate.pointer)
                        )
                        .trim_end(),
                    );
                    out.push('\n');
                }
                if route.action == IngressAction::Dispatch {
                    out.push_str(&format!(
                        "            -> dispatch {}\n",
                        quote(route.intent.as_deref().unwrap_or_default())
                    ));
                } else {
                    out.push_str(&format!(
                        "            -> {}\n",
                        ingress_action_name(route.action)
                    ));
                }
            }
            out.push_str("    }\n");
        }
        if let Some(orchestration) = spec.metadata.get("orchestration").and_then(|value| {
            serde_json::from_value::<OrchestrationPolicy>(value.clone().into()).ok()
        }) {
            out.push('\n');
            render_orchestration_policy(&mut out, &orchestration);
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
                        runinator_rexrap_codegen::render_expression(&link.parameters)
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
                        runinator_rexrap_codegen::render_expression(&join.parameters)
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

fn render_orchestration_policy(out: &mut String, policy: &OrchestrationPolicy) {
    out.push_str("    orchestration {\n");
    let mut intents = policy.intents.iter().collect::<Vec<_>>();
    intents.sort_by(|(left_name, left), (right_name, right)| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left_name.cmp(right_name))
    });
    for (name, intent) in intents {
        let effect = match intent.effect {
            ControlEffect::Terminate => "terminate",
            ControlEffect::Suspend => "suspend",
            ControlEffect::Resume => "resume",
            ControlEffect::Supersede => "supersede",
            ControlEffect::Observe => "observe",
            ControlEffect::Signal => "signal",
        };
        out.push_str(&format!(
            "        intent {} effect {effect} priority {}",
            quote(name),
            intent.priority
        ));
        if let Some(seconds) = intent.coalesce_seconds {
            out.push_str(&format!(" coalesce {}", render_duration(seconds)));
        }
        if intent.stop != EpochStopAction::Cancel {
            out.push_str(&format!(
                " stop {}",
                match intent.stop {
                    EpochStopAction::Pause => "pause",
                    EpochStopAction::None => "none",
                    EpochStopAction::Cancel => "cancel",
                }
            ));
        }
        match &intent.restart {
            RestartSelector::Entry => {}
            RestartSelector::Current => out.push_str(" restart current"),
            RestartSelector::Member(member) => out.push_str(&format!(" restart {}", quote(member))),
        }
        if let Some(pointer) = &intent.subject_revision_pointer {
            out.push_str(&format!(" revision {}", quote(pointer)));
        }
        if let Some(signal) = &intent.signal_name {
            out.push_str(&format!(" signal {}", quote(signal)));
        }
        if intent.allow_self_originated {
            out.push_str(" allow_self_originated");
        }
        out.push('\n');
    }
    for (name, budget) in &policy.budgets {
        let exhausted = match budget.exhausted {
            BudgetExhaustion::Fail => "fail",
            BudgetExhaustion::Pause => "pause",
            BudgetExhaustion::Terminate => "terminate",
        };
        out.push_str(&format!(
            "        budget {} attempts {} exhausted {exhausted}",
            quote(name),
            budget.attempts
        ));
        if let Some(handoff) = &budget.handoff {
            out.push_str(&format!(" via {}", quote(handoff)));
        }
        out.push('\n');
    }
    for (member, phase) in &policy.phases {
        out.push_str(&format!("        phase {} {{\n", quote(member)));
        for (field, pointer) in [
            ("subject_revision", &phase.result.subject_revision),
            ("resources", &phase.result.resources),
            ("evidence", &phase.result.evidence),
            ("failure_class", &phase.result.failure_class),
            ("correlations", &phase.result.correlations),
        ] {
            if let Some(pointer) = pointer {
                out.push_str(&format!("            {field} from {}\n", quote(pointer)));
            }
        }
        if let Some(workspace) = &phase.workspace {
            out.push_str(&format!(
                "            workspace scope {}",
                quote(&workspace.scope)
            ));
            if workspace.reuse {
                out.push_str(" reuse");
            }
            if workspace.lease_seconds != 300 {
                out.push_str(&format!(
                    " lease {}",
                    render_duration(workspace.lease_seconds)
                ));
            }
            if workspace.recovery != WorkspaceRecovery::Replace {
                out.push_str(&format!(
                    " recovery {}",
                    match workspace.recovery {
                        WorkspaceRecovery::Wait => "wait",
                        WorkspaceRecovery::Fail => "fail",
                        WorkspaceRecovery::Replace => "replace",
                    }
                ));
            }
            if workspace
                .requirements
                .as_object()
                .is_some_and(|values| !values.is_empty())
            {
                let labels = runinator_rexrap_codegen::render_expression(&workspace.requirements)
                    .unwrap_or_else(|_| "{}".into());
                out.push_str(&format!(" labels {labels}"));
            }
            out.push('\n');
        }
        out.push_str("        }\n");
    }
    out.push_str("    }\n");
}

fn render_duration(seconds: u64) -> String {
    if seconds.is_multiple_of(86_400) {
        format!("{}d", seconds / 86_400)
    } else if seconds.is_multiple_of(3_600) {
        format!("{}h", seconds / 3_600)
    } else if seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

fn ingress_lifecycle_name(lifecycle: IngressLifecycle) -> &'static str {
    lifecycle.as_str()
}

fn ingress_action_name(action: IngressAction) -> &'static str {
    action.as_str()
}

/// render a pipeline trigger spec back to `.rexrapp` source. mirrors `lower_trigger` so files round-trip.
fn render_trigger(trigger: &PipelineTriggerSpec) -> String {
    let config = &trigger.configuration;
    let disabled = if trigger.enabled { "" } else { " disabled" };
    if trigger.kind == WorkflowTriggerKind::Cron {
        if let Some(schedule) = config.get("schedule") {
            let schedule = runinator_rexrap_codegen::decompile::render_expression(schedule)
                .unwrap_or_else(|_| "{}".into());
            let mut rendered = format!("    trigger schedule {schedule}");
            if let Some(exclusions) = config.get("exclusions").and_then(Value::as_array) {
                for exclusion in exclusions {
                    let exclusion =
                        runinator_rexrap_codegen::decompile::render_expression(exclusion)
                            .unwrap_or_else(|_| "{}".into());
                    rendered.push_str(&format!(" blackout schedule {exclusion}"));
                }
            }
            rendered.push_str(disabled);
            rendered.push('\n');
            return rendered;
        }
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
