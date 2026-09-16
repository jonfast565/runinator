use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use runinator_models::{
    ai_usage::{AiUsageBreakdown, AiUsageRecord, AiUsageReport, AiUsageTotals},
    errors::SendableError,
};
use runinator_store::roles::AiUsageStore;
use uuid::Uuid;

fn report(records: Vec<AiUsageRecord>, workflow_report: bool) -> AiUsageReport {
    let mut totals = AiUsageTotals::default();
    let mut by_run = BTreeMap::<String, AiUsageTotals>::new();
    let mut by_node = BTreeMap::<String, AiUsageTotals>::new();
    let mut by_provider = BTreeMap::<String, AiUsageTotals>::new();
    let mut by_model = BTreeMap::<String, AiUsageTotals>::new();
    for record in &records {
        totals.add(record);
        by_run
            .entry(record.workflow_run_id.to_string())
            .or_default()
            .add(record);
        by_node
            .entry(
                record
                    .node_id
                    .clone()
                    .unwrap_or_else(|| "unattributed".into()),
            )
            .or_default()
            .add(record);
        by_provider
            .entry(record.provider.clone())
            .or_default()
            .add(record);
        by_model
            .entry(record.model.clone())
            .or_default()
            .add(record);
    }
    let breakdown = |values: BTreeMap<String, AiUsageTotals>| {
        values
            .into_iter()
            .map(|(key, totals)| AiUsageBreakdown { key, totals })
            .collect()
    };
    AiUsageReport {
        totals,
        records: if workflow_report { Vec::new() } else { records },
        by_run: if workflow_report {
            breakdown(by_run)
        } else {
            Vec::new()
        },
        by_node: breakdown(by_node),
        by_provider: breakdown(by_provider),
        by_model: breakdown(by_model),
    }
}

pub async fn ai_usage_for_run<T: AiUsageStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<AiUsageReport, SendableError> {
    Ok(report(
        db.fetch_ai_usage_for_run(workflow_run_id).await?,
        false,
    ))
}

pub async fn ai_usage_for_workflow<T: AiUsageStore>(
    db: &T,
    workflow_id: Uuid,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> Result<AiUsageReport, SendableError> {
    Ok(report(
        db.fetch_ai_usage_for_workflow(workflow_id, since, until)
            .await?,
        true,
    ))
}
