use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use runinator_models::{
    pipelines::{PipelineMemberAttempt, PipelineRunDetail},
    workflow_vm::{
        WorkflowEffect, WorkflowEffectRequest, WorkflowEffectStatus, WorkflowJournalEntry,
        WorkflowJournalRecord,
    },
    workflows::WorkflowRun,
};
use uuid::Uuid;

use crate::output;

pub(super) fn workflow_table(
    run: &WorkflowRun,
    journal: &[WorkflowJournalRecord],
    effects: &[WorkflowEffect],
) -> String {
    let effects = effects
        .iter()
        .map(|effect| (effect.id, effect))
        .collect::<HashMap<_, _>>();
    let rows = journal
        .iter()
        .filter_map(|record| workflow_event_row(record, &effects))
        .collect::<Vec<_>>();
    format!(
        "workflow run {} [{}]\n{}",
        run.id,
        run.status.as_str(),
        output::table(
            &["SEQ", "TIME", "EVENT", "NODE / DETAIL", "CONTINUATION"],
            &rows
        )
    )
}

fn workflow_event_row(
    record: &WorkflowJournalRecord,
    effects: &HashMap<Uuid, &WorkflowEffect>,
) -> Option<Vec<String>> {
    let (event, detail, continuation) = match &record.entry {
        // These are VM bookkeeping boundaries. JSON keeps them, while the terminal table stays at
        // the author-facing node/effect level.
        WorkflowJournalEntry::Entered { .. } | WorkflowJournalEntry::Transitioned { .. } => {
            return None;
        }
        WorkflowJournalEntry::NodeEntered {
            continuation_id,
            node_id,
        } => ("node", node_id.clone(), Some(*continuation_id)),
        WorkflowJournalEntry::Forked {
            continuation_id,
            children,
            join_key,
        } => (
            "fork",
            format!(
                "{} -> {}",
                join_key,
                children
                    .iter()
                    .map(|id| short_id(*id))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Some(*continuation_id),
        ),
        WorkflowJournalEntry::EffectRequested { effect_id, .. } => {
            let detail = effects
                .get(effect_id)
                .map(|effect| {
                    format!(
                        "{} [{}]",
                        effect_label(&effect.request),
                        effect_status(effect.status)
                    )
                })
                .unwrap_or_else(|| effect_id.to_string());
            (
                "effect requested",
                detail,
                effects.get(effect_id).map(|effect| effect.continuation_id),
            )
        }
        WorkflowJournalEntry::EffectSettled { effect_id, status } => (
            "effect settled",
            format!("{} [{}]", short_id(*effect_id), effect_status(*status)),
            effects.get(effect_id).map(|effect| effect.continuation_id),
        ),
        WorkflowJournalEntry::EffectRetryScheduled {
            effect_id,
            attempt,
            available_at,
        } => (
            "effect retry",
            format!(
                "{} attempt {} at {}",
                short_id(*effect_id),
                attempt,
                unix_time(*available_at)
            ),
            effects.get(effect_id).map(|effect| effect.continuation_id),
        ),
        WorkflowJournalEntry::Completed {
            continuation_id, ..
        } => (
            "completed",
            "continuation completed".into(),
            Some(*continuation_id),
        ),
        WorkflowJournalEntry::Failed {
            continuation_id,
            message,
            node_id,
        } => (
            "failed",
            match node_id {
                Some(node) => format!("{node}: {message}"),
                None => message.clone(),
            },
            Some(*continuation_id),
        ),
        WorkflowJournalEntry::Interrupted {
            continuation_id,
            handler_continuation_id,
            source,
        } => (
            "interrupted",
            format!(
                "{source:?} -> handler {}",
                short_id(*handler_continuation_id)
            ),
            Some(*continuation_id),
        ),
        WorkflowJournalEntry::InterruptResolved {
            continuation_id,
            handler_continuation_id,
            outcome,
        } => (
            "interrupt resolved",
            format!(
                "handler {}: {outcome:?}",
                short_id(*handler_continuation_id)
            ),
            Some(*continuation_id),
        ),
    };
    Some(vec![
        record.sequence.to_string(),
        unix_time(record.created_at),
        event.into(),
        output::truncate(&detail, 80),
        continuation.map(short_id).unwrap_or_else(|| "-".into()),
    ])
}

pub(super) fn workflow_graph(
    run: &WorkflowRun,
    journal: &[WorkflowJournalRecord],
    effects: &[WorkflowEffect],
) -> String {
    let mut lanes: HashMap<Uuid, Lane> = HashMap::new();
    let mut parents = HashMap::new();
    for record in journal {
        match &record.entry {
            WorkflowJournalEntry::NodeEntered {
                continuation_id,
                node_id,
            } => {
                let lane = lanes.entry(*continuation_id).or_insert_with(|| Lane {
                    id: *continuation_id,
                    first_sequence: record.sequence,
                    nodes: Vec::new(),
                });
                lane.nodes.push(node_id.clone());
            }
            WorkflowJournalEntry::Forked {
                continuation_id,
                children,
                ..
            } => {
                for child in children {
                    parents.insert(*child, *continuation_id);
                }
            }
            _ => {}
        }
    }
    let effect_statuses = effects
        .iter()
        .filter_map(|effect| {
            effect
                .node_id
                .as_ref()
                .map(|node| ((effect.continuation_id, node.clone()), effect.status))
        })
        .collect::<HashMap<_, _>>();
    let mut lanes = lanes.into_values().collect::<Vec<_>>();
    lanes.sort_by_key(|lane| lane.first_sequence);

    let mut text = format!("workflow run {} [{}]\n", run.id, run.status.as_str());
    if lanes.is_empty() {
        text.push_str("(no node execution recorded)\n");
        return text;
    }
    for lane in lanes {
        let depth = lane_depth(lane.id, &parents);
        let branch = if depth == 0 { "" } else { "`-- " };
        let nodes = lane
            .nodes
            .iter()
            .map(|node| {
                let status = effect_statuses
                    .get(&(lane.id, node.clone()))
                    .map(|status| format!(" [{}]", effect_status(*status)))
                    .or_else(|| {
                        (run.active_node_id.as_deref() == Some(node.as_str())
                            && !run.status.is_terminal())
                        .then(|| " [active]".into())
                    })
                    .unwrap_or_default();
                format!("{node}{status}")
            })
            .collect::<Vec<_>>()
            .join(" -> ");
        text.push_str(&format!(
            "{}{}{}: {}\n",
            "  ".repeat(depth),
            branch,
            short_id(lane.id),
            nodes
        ));
    }
    text
}

#[derive(Debug)]
struct Lane {
    id: Uuid,
    first_sequence: u64,
    nodes: Vec<String>,
}

fn lane_depth(id: Uuid, parents: &HashMap<Uuid, Uuid>) -> usize {
    let mut depth = 0;
    let mut current = id;
    let mut seen = HashSet::new();
    while let Some(parent) = parents.get(&current) {
        if !seen.insert(current) {
            break;
        }
        depth += 1;
        current = *parent;
    }
    depth
}

pub(super) fn pipeline_table(detail: &PipelineRunDetail) -> String {
    let mut attempts = detail.attempts.iter().collect::<Vec<_>>();
    attempts.sort_by_key(|attempt| attempt.created_at);
    let rows: Vec<Vec<String>> = if attempts.is_empty() {
        detail
            .members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                vec![
                    (index + 1).to_string(),
                    member.workflow_id.to_string(),
                    "1".into(),
                    member.status.as_str().into(),
                    member.id.to_string(),
                    output::time(member.started_at),
                    output::time(member.finished_at),
                    elapsed(member.started_at, member.finished_at),
                    member.message.clone().unwrap_or_default(),
                ]
            })
            .collect()
    } else {
        attempts
            .into_iter()
            .enumerate()
            .map(|(index, attempt)| pipeline_attempt_row(index, attempt))
            .collect()
    };
    format!(
        "pipeline run {} [{}]\n{}",
        detail.run.id,
        detail.run.status.as_str(),
        output::table(
            &[
                "SEQ",
                "MEMBER",
                "TRY",
                "STATUS",
                "WORKFLOW RUN",
                "STARTED",
                "FINISHED",
                "ELAPSED",
                "MESSAGE"
            ],
            &rows,
        )
    )
}

fn pipeline_attempt_row(index: usize, attempt: &PipelineMemberAttempt) -> Vec<String> {
    vec![
        (index + 1).to_string(),
        attempt.member_key.clone(),
        attempt.attempt.to_string(),
        attempt.status.as_str().into(),
        attempt
            .workflow_run_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".into()),
        output::time(attempt.started_at),
        output::time(attempt.finished_at),
        elapsed(attempt.started_at, attempt.finished_at),
        attempt.message.clone().unwrap_or_default(),
    ]
}

pub(super) fn pipeline_graph(detail: &PipelineRunDetail) -> String {
    let mut text = format!(
        "pipeline run {} [{}]\n",
        detail.run.id,
        detail.run.status.as_str()
    );
    let latest = latest_attempts(&detail.attempts);
    let Some(snapshot) = detail.run.pipeline_snapshot.as_ref() else {
        if latest.is_empty() {
            if detail.members.is_empty() {
                text.push_str("(pipeline snapshot and member attempts unavailable)\n");
            } else {
                for member in &detail.members {
                    text.push_str(&format!(
                        "[{}] workflow {}\n",
                        member.status.as_str(),
                        member.workflow_id
                    ));
                }
            }
        } else {
            let mut attempts = latest.into_values().collect::<Vec<_>>();
            attempts.sort_by(|left, right| left.member_key.cmp(&right.member_key));
            for attempt in attempts {
                text.push_str(&format!(
                    "[{}] {} (attempt {})\n",
                    attempt.status.as_str(),
                    attempt.member_key,
                    attempt.attempt
                ));
            }
        }
        return text;
    };

    let mut statuses = snapshot
        .graph
        .members
        .iter()
        .filter_map(|member| {
            detail
                .members
                .iter()
                .filter(|run| run.workflow_id == member.workflow_id)
                .max_by_key(|run| run.created_at)
                .map(|run| (member.key.as_str(), run.status.as_str()))
        })
        .collect::<HashMap<_, _>>();
    for (member, attempt) in &latest {
        statuses.insert(member, attempt.status.as_str());
    }

    let linked = snapshot
        .graph
        .links
        .iter()
        .flat_map(|link| [link.from.as_str(), link.to.as_str()])
        .collect::<HashSet<_>>();
    for member in &snapshot.graph.members {
        text.push_str(&format!(
            "[{}] {}{}\n",
            member_status(&member.key, &statuses),
            member.key,
            member_attempt_suffix(&member.key, &latest)
        ));
        let outgoing = snapshot
            .graph
            .links
            .iter()
            .filter(|link| link.from == member.key)
            .collect::<Vec<_>>();
        for (index, link) in outgoing.iter().enumerate() {
            let connector = if index + 1 == outgoing.len() {
                "`--"
            } else {
                "+--"
            };
            let enabled = if link.enabled { "" } else { " disabled" };
            text.push_str(&format!(
                "  {connector}({}{enabled})--> [{}] {}{}\n",
                link.on.as_str(),
                member_status(&link.to, &statuses),
                link.to,
                member_attempt_suffix(&link.to, &latest)
            ));
        }
        if outgoing.is_empty() && !linked.contains(member.key.as_str()) {
            text.push_str("  `--(standalone)\n");
        }
    }
    if !detail.joins.is_empty() {
        text.push_str("joins:\n");
        for join in &detail.joins {
            text.push_str(&format!(
                "  {} [{}] {}/{} inputs\n",
                join.target, join.state, join.satisfied_inputs, join.total_inputs
            ));
        }
    }
    text
}

fn latest_attempts(attempts: &[PipelineMemberAttempt]) -> HashMap<&str, &PipelineMemberAttempt> {
    let mut latest: HashMap<&str, &PipelineMemberAttempt> = HashMap::new();
    for attempt in attempts {
        if latest
            .get(attempt.member_key.as_str())
            .is_none_or(|current| attempt.attempt > current.attempt)
        {
            latest.insert(&attempt.member_key, attempt);
        }
    }
    latest
}

fn member_status(member: &str, statuses: &HashMap<&str, &'static str>) -> &'static str {
    statuses.get(member).copied().unwrap_or("pending")
}

fn member_attempt_suffix(member: &str, latest: &HashMap<&str, &PipelineMemberAttempt>) -> String {
    latest
        .get(member)
        .map(|attempt| format!(" (attempt {})", attempt.attempt))
        .unwrap_or_default()
}

fn effect_label(request: &WorkflowEffectRequest) -> String {
    match request {
        WorkflowEffectRequest::Action {
            provider, function, ..
        } => format!("{provider}.{function}"),
        WorkflowEffectRequest::Timer { due_at } => format!("timer until {}", unix_time(*due_at)),
        WorkflowEffectRequest::TimerDelay { seconds } => format!("delay {seconds}s"),
        WorkflowEffectRequest::Approval { .. } => "approval".into(),
        WorkflowEffectRequest::Gate { kind, .. } => format!("{kind:?} gate"),
        WorkflowEffectRequest::Signal { key, .. } => format!("signal {key}"),
        WorkflowEffectRequest::Input { .. } => "input".into(),
        WorkflowEffectRequest::EventWait { event_type, .. } => format!("event {event_type}"),
        WorkflowEffectRequest::ChildRun {
            workflow_name,
            workflow_id,
            ..
        } => format!(
            "child workflow {}",
            workflow_name
                .clone()
                .or_else(|| workflow_id.map(|id| id.to_string()))
                .unwrap_or_else(|| "-".into())
        ),
        WorkflowEffectRequest::AwaitRun { workflow, .. } => format!("await workflow {workflow}"),
        WorkflowEffectRequest::MutexAcquire { key } => format!("mutex {key}"),
        WorkflowEffectRequest::Coordination { kind, .. } => kind.clone(),
    }
}

fn effect_status(status: WorkflowEffectStatus) -> &'static str {
    match status {
        WorkflowEffectStatus::Requested => "requested",
        WorkflowEffectStatus::Running => "running",
        WorkflowEffectStatus::Succeeded => "succeeded",
        WorkflowEffectStatus::Failed => "failed",
        WorkflowEffectStatus::Rejected => "rejected",
        WorkflowEffectStatus::TimedOut => "timed_out",
        WorkflowEffectStatus::Canceled => "canceled",
    }
}

fn unix_time(seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| seconds.to_string())
}

fn elapsed(started: Option<DateTime<Utc>>, finished: Option<DateTime<Utc>>) -> String {
    let Some(started) = started else {
        return "-".into();
    };
    let finished = finished.unwrap_or_else(Utc::now);
    let milliseconds = (finished - started).num_milliseconds().max(0);
    if milliseconds < 1_000 {
        return format!("{milliseconds}ms");
    }
    let seconds = milliseconds / 1_000;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn short_id(id: Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_depth_stops_at_cycles() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let parents = HashMap::from([(first, second), (second, first)]);
        assert_eq!(lane_depth(first, &parents), 2);
    }

    #[test]
    fn elapsed_uses_compact_terminal_units() {
        let started = DateTime::from_timestamp(1_000, 0);
        let finished = DateTime::from_timestamp(4_661, 0);
        assert_eq!(elapsed(started, finished), "1h01m01s");
    }

    #[test]
    fn short_ids_are_readable_and_stable() {
        assert_eq!(short_id(Uuid::from_u128(0x1234)), "00000000");
    }
}
