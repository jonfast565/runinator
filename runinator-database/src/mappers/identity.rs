use super::*;

macro_rules! user_from_row {
    ($row:expr) => {
        User {
            id: Some($row.get::<Uuid, _>("id")),
            username: $row.get::<String, _>("username"),
            email: $row.get::<Option<String>, _>("email"),
            disabled: $row.get::<bool, _>("disabled"),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    };
}

macro_rules! api_key_from_row {
    ($row:expr) => {
        ApiKey {
            id: Some($row.get::<Uuid, _>("id")),
            name: $row.get::<String, _>("name"),
            principal_kind: PrincipalKind::from_str_lossy(&$row.get::<String, _>("principal_kind"))
                .unwrap_or(PrincipalKind::User),
            principal_id: $row.get::<Uuid, _>("principal_id"),
            system_role: $row
                .get::<Option<String>, _>("system_role")
                .and_then(|value| runinator_models::rbac::SystemRole::from_str_lossy(&value)),
            org_id: $row.get::<Option<Uuid>, _>("org_id"),
            action_ceiling: $row
                .get::<Option<String>, _>("action_ceiling_json")
                .and_then(|value| serde_json::from_str(&value).ok())
                .unwrap_or_default(),
            key_prefix: $row.get::<String, _>("key_prefix"),
            last_used_at: $row
                .get::<Option<i64>, _>("last_used_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            expires_at: $row
                .get::<Option<i64>, _>("expires_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            disabled: $row.get::<bool, _>("disabled"),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    };
}

row_mapper!(row_to_user(row) -> User { user_from_row!(row) });

row_mapper!(row_to_local_credential(row) -> LocalCredential {
    LocalCredential {
        user: user_from_row!(row),
        password_hash: row.get::<String, _>("password_hash"),
    }
});

row_mapper!(row_to_api_key(row) -> ApiKey { api_key_from_row!(row) });

row_mapper!(row_to_api_key_record(row) -> ApiKeyRecord {
    ApiKeyRecord {
        key: api_key_from_row!(row),
        key_hash: row.get::<String, _>("key_hash"),
    }
});

row_mapper!(row_to_agent_enrollment_token_record(row) -> AgentEnrollmentTokenRecord {
    AgentEnrollmentTokenRecord {
        token: AgentEnrollmentToken {
            token_id: row.get::<String, _>("token_id"),
            org_id: row.get::<Option<Uuid>, _>("org_id"),
            labels: serde_json::from_str(&row.get::<String, _>("labels_json"))
                .unwrap_or_default(),
            service_url: row.get::<String, _>("service_url"),
            spki_pin: row.get::<Option<String>, _>("spki_pin"),
            expires_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("expires_at"), 0)
                .unwrap_or_else(Utc::now),
            consumed_at: row
                .get::<Option<i64>, _>("consumed_at")
                .and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
            issued_by: row.get::<Option<Uuid>, _>("issued_by"),
            created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        },
        sealed_secret: row.get::<Vec<u8>, _>("sealed_secret"),
    }
});

row_mapper!(row_to_auth_session(row) -> AuthSession {
    AuthSession {
        id: row.get::<Uuid, _>("id"),
        user_id: row.get::<Uuid, _>("user_id"),
        refresh_token_hash: row.get::<String, _>("refresh_token_hash"),
        expires_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("expires_at"), 0)
            .unwrap_or_else(Utc::now),
        revoked: row.get::<bool, _>("revoked"),
        refresh_count: row.get::<i64, _>("refresh_count"),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
        last_seen_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("last_seen_at"), 0)
            .unwrap_or_else(Utc::now),
        user_agent: row.get::<Option<String>, _>("user_agent"),
        ip_address: row.get::<Option<String>, _>("ip_address"),
    }
});

row_mapper!(row_to_team(row) -> Team {
    Team {
        id: Some(row.get::<Uuid, _>("id")),
        name: row.get::<String, _>("name"),
        scope: ScopeRef::new(
            ScopeKind::from_str_lossy(&row.get::<String, _>("scope_kind"))
                .unwrap_or(ScopeKind::Platform),
            row.get::<Option<Uuid>, _>("scope_id"),
        ).unwrap_or(ScopeRef::PLATFORM),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_organization(row) -> Organization {
    Organization {
        id: Some(row.get::<Uuid, _>("id")),
        name: row.get::<String, _>("name"),
        slug: row.get::<String, _>("slug"),
        disabled: row.get::<bool, _>("disabled"),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
        updated_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("updated_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_org_membership(row) -> OrgMembership {
    OrgMembership {
        org_id: row.get::<Uuid, _>("org_id"),
        user_id: row.get::<Uuid, _>("user_id"),
        role: OrgRole::from_str_lossy(&row.get::<String, _>("role")).unwrap_or(OrgRole::Member),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_org_quota(row) -> OrgQuota {
    OrgQuota {
        org_id: row.get::<Uuid, _>("org_id"),
        max_nodes_per_kind: serde_json::from_str(&row.get::<String, _>("max_nodes_json"))
            .unwrap_or_default(),
        max_monthly_cents: row.get::<i64, _>("max_monthly_cents") as u32,
    }
});

row_mapper!(row_to_usage_sample(row) -> UsageSample {
    UsageSample {
        org_id: row.get::<Uuid, _>("org_id"),
        backend: ProvisionBackend::try_from(row.get::<String, _>("backend").as_str())
            .unwrap_or(ProvisionBackend::Supervisor),
        kind: ReplicaKind::try_from(row.get::<String, _>("kind").as_str())
            .unwrap_or(ReplicaKind::Worker),
        node_count: row.get::<i64, _>("node_count") as u32,
        sampled_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("sampled_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_org_resource_group(row) -> OrgResourceGroup {
    OrgResourceGroup {
        org_id: row.get::<Uuid, _>("org_id"),
        backend: ProvisionBackend::try_from(row.get::<String, _>("backend").as_str())
            .unwrap_or(ProvisionBackend::Supervisor),
        kind: ReplicaKind::try_from(row.get::<String, _>("kind").as_str())
            .unwrap_or(ReplicaKind::Worker),
        desired: row.get::<i64, _>("desired") as u32,
        dedicated: row.get::<bool, _>("dedicated"),
    }
});

row_mapper!(row_to_grant(row) -> Grant {
    Grant {
        id: Some(row.get::<Uuid, _>("id")),
        resource_type: ResourceType::from_str_lossy(&row.get::<String, _>("resource_type"))
            .unwrap_or(ResourceType::Workflow),
        resource_id: row.get::<Uuid, _>("resource_id"),
        principal_type: PrincipalType::from_str_lossy(&row.get::<String, _>("principal_type"))
            .unwrap_or(PrincipalType::User),
        principal_id: row.get::<Uuid, _>("principal_id"),
        permission: Permission::from_str_lossy(&row.get::<String, _>("permission"))
            .unwrap_or(Permission::View),
        created_at: DateTime::<Utc>::from_timestamp(row.get::<i64, _>("created_at"), 0)
            .unwrap_or_else(Utc::now),
    }
});
