use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use runinator_models::billing::{
    OrgQuota, OrgResourceGroup, OrgUsage, RateCard, ScaleOrgNodesRequest, UpdateOrgQuotaRequest,
    UsageSample,
};
use runinator_models::provisioning::{NodeSpec, ProvisionBackend};
use runinator_models::replicas::ReplicaKind;
use runinator_models::value::Value;
use runinator_provisioner::ProvisionerRegistry;
use serde::Serialize;
use uuid::Uuid;

use crate::authz;
use crate::models::{ApiError, ApiResponse};
use crate::responses::{api_error, bad_request};

type Reply = (StatusCode, Json<ApiResponse>);

/// billing month approximation used to project a monthly cost from an hourly rate.
pub(crate) const HOURS_PER_MONTH: u64 = 730;

fn ok_value<T: Serialize>(value: &T) -> Reply {
    match serde_json::to_value(value) {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(Value::from(value))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

fn quota_error(message: impl Into<String>) -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(ApiError::new(&message.into()))),
    )
}

/// the platform rate card. a fixed default today; a settings-backed override is a follow-up.
fn rate_card() -> RateCard {
    RateCard::default_card()
}

/// project the monthly cost (cents) of a set of allocations under the rate card.
pub(crate) fn projected_monthly_cents(groups: &[OrgResourceGroup], card: &RateCard) -> u64 {
    groups
        .iter()
        .map(|g| g.desired as u64 * card.hourly_cents(g.backend, g.kind) as u64 * HOURS_PER_MONTH)
        .sum()
}

/// the platform rate card (any authenticated principal may read it).
pub(crate) async fn get_rate_card() -> Reply {
    ok_value(&rate_card())
}

/// an org's dedicated allocations, each annotated with its projected monthly cost.
pub(crate) async fn get_org_nodes<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = authz::require_org_member(&ctx, org_id) {
        return reply;
    }
    let groups = match db.list_org_resource_groups(org_id).await {
        Ok(groups) => groups,
        Err(err) => return api_error(err.to_string()),
    };
    let card = rate_card();
    let monthly = projected_monthly_cents(&groups, &card);
    ok_value(&serde_json::json!({
        "groups": groups,
        "projected_monthly_cents": monthly,
    }))
}

/// scale an org's dedicated allocation for a (backend, kind), enforcing quota, then scale the org's
/// own labeled node pool (`org-<slug>-<kind>`) to match so its work routes to dedicated workers.
pub(crate) async fn scale_org_nodes<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(registry): Extension<Arc<ProvisionerRegistry>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
    Json(request): Json<ScaleOrgNodesRequest>,
) -> Reply {
    if let Err(reply) = authz::require_org_admin(&ctx, org_id) {
        return reply;
    }
    let card = rate_card();
    let quota = match db.fetch_org_quota(org_id).await {
        Ok(quota) => quota,
        Err(err) => return api_error(err.to_string()),
    };

    // per-kind node cap.
    if let Some(quota) = &quota {
        if let Some(cap) = quota.max_nodes(request.kind) {
            if request.desired > cap {
                return quota_error(format!(
                    "requested {} {} node(s) exceeds the org cap of {cap}",
                    request.desired,
                    request.kind.as_str()
                ));
            }
        }
    }

    // monthly budget cap: project the org's spend after this change across all its allocations.
    let mut groups = match db.list_org_resource_groups(org_id).await {
        Ok(groups) => groups,
        Err(err) => return api_error(err.to_string()),
    };
    apply_allocation(
        &mut groups,
        org_id,
        request.backend,
        request.kind,
        request.desired,
    );
    if let Some(quota) = &quota {
        if quota.max_monthly_cents > 0 {
            let projected = projected_monthly_cents(&groups, &card);
            if projected > quota.max_monthly_cents as u64 {
                return quota_error(format!(
                    "projected monthly cost {}¢ exceeds the org budget of {}¢",
                    projected, quota.max_monthly_cents
                ));
            }
        }
    }

    // record the allocation, then scale this org's dedicated, labeled pool to match.
    let group = OrgResourceGroup {
        org_id,
        backend: request.backend,
        kind: request.kind,
        desired: request.desired,
        dedicated: true,
    };
    if let Err(err) = db.upsert_org_resource_group(group.clone()).await {
        return api_error(err.to_string());
    }
    let slug = match db.fetch_org(org_id).await {
        Ok(Some(org)) => org.slug,
        Ok(None) => return api_error("organization not found"),
        Err(err) => return api_error(err.to_string()),
    };
    scale_org_pool(
        &registry,
        &slug,
        request.backend,
        request.kind,
        request.desired,
    )
    .await;
    ok_value(&group)
}

/// an org's quota (or an unset default), viewable by any org member.
pub(crate) async fn get_org_quota<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = authz::require_org_member(&ctx, org_id) {
        return reply;
    }
    let quota = match db.fetch_org_quota(org_id).await {
        Ok(Some(quota)) => quota,
        Ok(None) => OrgQuota {
            org_id,
            ..Default::default()
        },
        Err(err) => return api_error(err.to_string()),
    };
    ok_value(&quota)
}

/// set an org's quota (platform admin only — quotas are a platform-level cap on tenants).
pub(crate) async fn put_org_quota<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
    Json(request): Json<UpdateOrgQuotaRequest>,
) -> Reply {
    if let Err(reply) = authz::require_admin(&ctx) {
        return reply;
    }
    // reject unknown replica-kind keys so a typo never silently disables a cap.
    for key in request.max_nodes_per_kind.keys() {
        if ReplicaKind::try_from(key.as_str()).is_err() {
            return bad_request(format!(
                "unknown replica kind '{key}' in max_nodes_per_kind"
            ));
        }
    }
    let quota = OrgQuota {
        org_id,
        max_nodes_per_kind: request.max_nodes_per_kind,
        max_monthly_cents: request.max_monthly_cents,
    };
    match db.upsert_org_quota(quota).await {
        Ok(quota) => ok_value(&quota),
        Err(err) => api_error(err.to_string()),
    }
}

/// an org's accrued usage and cost since a rolling 30-day window.
pub(crate) async fn get_org_usage<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = authz::require_org_member(&ctx, org_id) {
        return reply;
    }
    let since = chrono::Utc::now() - chrono::Duration::days(30);
    let samples = match db.fetch_usage_samples(org_id, since.timestamp()).await {
        Ok(samples) => samples,
        Err(err) => return api_error(err.to_string()),
    };
    let usage = integrate_usage(org_id, samples, &rate_card());
    ok_value(&usage)
}

// insert or replace the (backend, kind) allocation for an org within an in-memory list.
fn apply_allocation(
    groups: &mut Vec<OrgResourceGroup>,
    org_id: Uuid,
    backend: ProvisionBackend,
    kind: ReplicaKind,
    desired: u32,
) {
    if let Some(existing) = groups
        .iter_mut()
        .find(|g| g.backend == backend && g.kind == kind)
    {
        existing.desired = desired;
        return;
    }
    groups.push(OrgResourceGroup {
        org_id,
        backend,
        kind,
        desired,
        dedicated: true,
    });
}

/// the backend-local group name for an org's dedicated pool of a kind (e.g. `org-acme-worker`).
pub(crate) fn org_pool_group(slug: &str, kind: ReplicaKind) -> String {
    format!("org-{slug}-{}", kind.as_str())
}

// scale an org's dedicated, org-labeled pool to `desired`. best-effort: when the backend is not
// configured (e.g. in tests) the recorded allocation still stands and this is a no-op.
async fn scale_org_pool(
    registry: &ProvisionerRegistry,
    slug: &str,
    backend: ProvisionBackend,
    kind: ReplicaKind,
    desired: u32,
) {
    let Some(provisioner) = registry.get(backend) else {
        return;
    };
    // label spawned nodes `org=<slug>` so the reducer can route this org's work to them, and put them
    // in the org's own group so pools scale independently of other tenants.
    let mut spec = NodeSpec::default();
    spec.group = Some(org_pool_group(slug, kind));
    spec.labels.insert("org".to_string(), slug.to_string());
    if let Err(err) = provisioner.scale(kind, desired, &spec).await {
        log::warn!(
            "failed to scale org '{slug}' {} pool to {desired}: {err}",
            kind.as_str()
        );
    }
}

/// integrate a time-ordered ledger into node-hours and accrued cost via left-Riemann sums: each
/// sample's node count is held constant over the interval until the next sample of the same group.
pub(crate) fn integrate_usage(
    org_id: Uuid,
    samples: Vec<UsageSample>,
    card: &RateCard,
) -> OrgUsage {
    let since = samples.first().map(|s| s.sampled_at);
    // bucket samples per (backend, kind) keyed by their string reps (the enums are not Ord/Hash),
    // preserving time order within each bucket.
    let mut buckets: BTreeMap<(String, String), Vec<&UsageSample>> = BTreeMap::new();
    for sample in &samples {
        buckets
            .entry((
                sample.backend.as_str().to_string(),
                sample.kind.as_str().to_string(),
            ))
            .or_default()
            .push(sample);
    }
    let mut node_hours: BTreeMap<String, f64> = BTreeMap::new();
    let mut accrued_cents = 0.0_f64;
    for (_, series) in buckets {
        let Some(first) = series.first() else {
            continue;
        };
        let (backend, kind) = (first.backend, first.kind);
        let hourly = card.hourly_cents(backend, kind) as f64;
        let mut hours = 0.0_f64;
        for pair in series.windows(2) {
            let dt = (pair[1].sampled_at - pair[0].sampled_at)
                .num_seconds()
                .max(0) as f64;
            hours += pair[0].node_count as f64 * (dt / 3600.0);
        }
        if hours > 0.0 {
            *node_hours.entry(kind.as_str().to_string()).or_default() += hours;
            accrued_cents += hours * hourly;
        }
    }
    OrgUsage {
        org_id,
        since,
        node_hours,
        accrued_cents: accrued_cents.round() as u64,
    }
}
