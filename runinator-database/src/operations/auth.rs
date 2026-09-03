//! users, API keys, sessions, teams, and grants.
//!
//! the `AuthStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;
use runinator_models::rbac::PlatformRole;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> AuthStore for SqlStore<B>
where
    B: SqlBackend,
    // encode bounds for every bound value type.
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Vec<u8>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    // decode bounds (operations read a couple of columns directly; mappers read the rest).
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    // row indexing + executor plumbing.
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn create_user(
        &self,
        username: String,
        email: Option<String>,
        password_hash: Option<String>,
    ) -> Result<User, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO users (id, username, email, disabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(&username)
        .bind(&email)
        .bind(false)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        if let Some(hash) = password_hash {
            sqlx::query(&self.render(
                "INSERT INTO user_identities (id, user_id, provider, subject, password_hash, created_at) VALUES (?, ?, 'local', ?, ?, ?)",
            ))
            .bind(Uuid::now_v7())
            .bind(id)
            .bind(&username)
            .bind(&hash)
            .bind(now)
            .execute(self.pool())
            .await?;
        }
        let at = DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now);
        Ok(User {
            id: Some(id),
            username,
            email,
            disabled: false,
            created_at: at,
            updated_at: at,
        })
    }

    async fn create_user_with_platform_role(
        &self,
        username: String,
        email: Option<String>,
        password_hash: Option<String>,
        role: PlatformRole,
        created_by: Option<Uuid>,
    ) -> Result<User, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(
            "INSERT INTO users (id, username, email, disabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(&username)
        .bind(&email)
        .bind(false)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if let Some(hash) = password_hash {
            sqlx::query(&self.render(
                "INSERT INTO user_identities (id, user_id, provider, subject, password_hash, created_at) VALUES (?, ?, 'local', ?, ?, ?)",
            ))
            .bind(Uuid::now_v7())
            .bind(id)
            .bind(&username)
            .bind(hash)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(&self.render(
            "INSERT INTO role_assignments (principal_kind, principal_id, scope_key, role, created_by, created_at, updated_at) \
             VALUES ('user', ?, 'platform', ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(role.as_str())
        .bind(created_by)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let at = DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now);
        Ok(User {
            id: Some(id),
            username,
            email,
            disabled: false,
            created_at: at,
            updated_at: at,
        })
    }

    async fn fetch_user(&self, id: Uuid) -> Result<Option<User>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, username, email, disabled, created_at, updated_at FROM users WHERE id = ?",
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_user(&row)))
    }

    async fn fetch_user_by_username(
        &self,
        username: String,
    ) -> Result<Option<User>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, username, email, disabled, created_at, updated_at FROM users WHERE username = ?",
        ))
        .bind(username)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_user(&row)))
    }

    async fn fetch_local_credential(
        &self,
        username: String,
    ) -> Result<Option<LocalCredential>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT u.id, u.username, u.email, u.disabled, u.created_at, u.updated_at, i.password_hash \
             FROM users u JOIN user_identities i ON i.user_id = u.id \
             WHERE i.provider = 'local' AND i.subject = ? AND i.password_hash IS NOT NULL",
        ))
        .bind(username)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_local_credential(&row)))
    }

    async fn list_users(&self) -> Result<Vec<User>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, username, email, disabled, created_at, updated_at FROM users ORDER BY username",
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_user).collect())
    }

    async fn count_users(&self) -> Result<i64, SendableError> {
        let row = sqlx::query(&self.render("SELECT COUNT(*) AS user_count FROM users"))
            .fetch_one(self.pool())
            .await?;
        Ok(row.get::<i64, _>("user_count"))
    }

    async fn update_user(
        &self,
        id: Uuid,
        email: Option<Option<String>>,
        disabled: Option<bool>,
    ) -> Result<User, SendableError> {
        let Some(current) = self.fetch_user(id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("User {id} not found"),
            )));
        };
        let email = email.unwrap_or(current.email);
        let disabled = disabled.unwrap_or(current.disabled);
        let now = Utc::now().timestamp();
        let mut tx = self.pool().begin().await?;
        if disabled && !current.disabled {
            if self.dialect() == SqlDialect::Sqlite {
                sqlx::query(&self.render(
                    "UPDATE role_assignments SET updated_at = updated_at WHERE scope_key IN (\
                     SELECT scope_key FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ?)",
                ))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(&self.render(
                    "SELECT principal_id FROM role_assignments WHERE scope_key IN (\
                     SELECT scope_key FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ?) FOR UPDATE",
                ))
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
            }
            let orphaned_scopes: i64 = sqlx::query_scalar(&self.render(
                "SELECT COUNT(*) FROM role_assignments target WHERE target.principal_kind = 'user' AND target.principal_id = ? \
                 AND ((target.scope_key = 'platform' AND target.role = 'admin') OR (target.scope_key <> 'platform' AND target.role = 'owner')) \
                 AND NOT EXISTS (SELECT 1 FROM role_assignments other WHERE other.scope_key = target.scope_key \
                   AND other.role = target.role \
                   AND (other.principal_kind <> target.principal_kind OR other.principal_id <> target.principal_id) AND (\
                     (other.principal_kind = 'user' AND EXISTS (SELECT 1 FROM users u WHERE u.id = other.principal_id AND u.disabled = ?)) OR \
                     (other.principal_kind = 'service' AND EXISTS (SELECT 1 FROM service_accounts s WHERE s.id = other.principal_id AND s.disabled = ?))))",
            ))
            .bind(id)
            .bind(false)
            .bind(false)
            .fetch_one(&mut *tx)
            .await?;
            if orphaned_scopes > 0 {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "the last enabled platform administrator or scope owner cannot be disabled",
                )));
            }
        }
        sqlx::query(
            &self.render("UPDATE users SET email = ?, disabled = ?, updated_at = ? WHERE id = ?"),
        )
        .bind(&email)
        .bind(disabled)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(User {
            id: Some(id),
            username: current.username,
            email,
            disabled,
            created_at: current.created_at,
            updated_at: DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now),
        })
    }

    async fn set_local_password(
        &self,
        user_id: Uuid,
        password_hash: String,
    ) -> Result<(), SendableError> {
        let Some(user) = self.fetch_user(user_id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("User {user_id} not found"),
            )));
        };
        // replace any existing local identity so the row stays unique on (provider, subject).
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            sqlx::query(
                &self.render("DELETE FROM user_identities WHERE user_id = ? AND provider = 'local'"),
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&self.render(
                "INSERT INTO user_identities (id, user_id, provider, subject, password_hash, created_at) VALUES (?, ?, 'local', ?, ?, ?)",
            ))
            .bind(Uuid::now_v7())
            .bind(user_id)
            .bind(user.username.as_str())
            .bind(password_hash.as_str())
            .bind(Utc::now().timestamp())
            .execute(&mut *tx)
            .await?;
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn delete_user(&self, id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            if self.dialect() == SqlDialect::Sqlite {
                sqlx::query(&self.render(
                    "UPDATE role_assignments SET updated_at = updated_at WHERE scope_key IN (\
                     SELECT scope_key FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ?)",
                ))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(&self.render(
                    "SELECT principal_id FROM role_assignments WHERE scope_key IN (\
                     SELECT scope_key FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ?) FOR UPDATE",
                ))
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
            }
            let orphaned_scopes: i64 = sqlx::query_scalar(&self.render(
                "SELECT COUNT(*) FROM role_assignments target WHERE target.principal_kind = 'user' AND target.principal_id = ? \
                 AND ((target.scope_key = 'platform' AND target.role = 'admin') OR (target.scope_key <> 'platform' AND target.role = 'owner')) \
                 AND NOT EXISTS (SELECT 1 FROM role_assignments other WHERE other.scope_key = target.scope_key \
                   AND other.role = target.role \
                   AND (other.principal_kind <> target.principal_kind OR other.principal_id <> target.principal_id) AND (\
                     (other.principal_kind = 'user' AND EXISTS (SELECT 1 FROM users u WHERE u.id = other.principal_id AND u.disabled = ?)) OR \
                     (other.principal_kind = 'service' AND EXISTS (SELECT 1 FROM service_accounts s WHERE s.id = other.principal_id AND s.disabled = ?))))",
            ))
            .bind(id)
            .bind(false)
            .bind(false)
            .fetch_one(&mut *tx)
            .await?;
            if orphaned_scopes > 0 {
                return Err(sqlx::Error::Protocol(
                    "the last enabled platform administrator or scope owner cannot be deleted"
                        .to_string(),
                ));
            }
            for sql in [
                "DELETE FROM auth_sessions WHERE user_id = ?",
                "DELETE FROM user_identities WHERE user_id = ?",
                "DELETE FROM resource_grants WHERE principal_type = 'user' AND principal_id = ?",
                "DELETE FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ?",
                "DELETE FROM users WHERE id = ?",
            ] {
                sqlx::query(&self.render(sql))
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn create_api_key(&self, record: ApiKeyRecord) -> Result<ApiKey, SendableError> {
        let id = record.key.id.unwrap_or_else(Uuid::now_v7);
        let created = record.key.created_at.timestamp();
        let last_used = record.key.last_used_at.map(|t| t.timestamp());
        let expires = record.key.expires_at.map(|t| t.timestamp());
        sqlx::query(&self.render(
            "INSERT INTO api_keys (id, name, principal_kind, principal_id, system_role, org_id, action_ceiling_json, key_prefix, key_hash, last_used_at, expires_at, disabled, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(&record.key.name)
        .bind(record.key.principal_kind.as_str())
        .bind(record.key.principal_id)
        .bind(record.key.system_role.map(|role| role.as_str().to_string()))
        .bind(record.key.org_id)
        .bind(serde_json::to_string(&record.key.action_ceiling)?)
        .bind(&record.key.key_prefix)
        .bind(&record.key_hash)
        .bind(last_used)
        .bind(expires)
        .bind(record.key.disabled)
        .bind(created)
        .execute(self.pool())
        .await?;
        let mut stored = record.key;
        stored.id = Some(id);
        Ok(stored)
    }

    async fn fetch_api_key(&self, id: Uuid) -> Result<Option<ApiKeyRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, name, principal_kind, principal_id, system_role, org_id, action_ceiling_json, key_prefix, key_hash, last_used_at, expires_at, disabled, created_at FROM api_keys WHERE id = ?",
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_api_key_record(&row)))
    }

    async fn fetch_api_key_by_prefix(
        &self,
        prefix: String,
    ) -> Result<Option<ApiKeyRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, name, principal_kind, principal_id, system_role, org_id, action_ceiling_json, key_prefix, key_hash, last_used_at, expires_at, disabled, created_at FROM api_keys WHERE key_prefix = ?",
        ))
        .bind(prefix)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_api_key_record(&row)))
    }

    async fn list_api_keys(&self, user_id: Option<Uuid>) -> Result<Vec<ApiKey>, SendableError> {
        let columns = "id, name, principal_kind, principal_id, system_role, org_id, action_ceiling_json, key_prefix, last_used_at, expires_at, disabled, created_at";
        let rows =
            match user_id {
                Some(uid) => sqlx::query(&self.render(&format!(
                    "SELECT {columns} FROM api_keys WHERE principal_id = ? ORDER BY created_at DESC"
                )))
                .bind(uid)
                .fetch_all(self.pool())
                .await?,
                None => {
                    sqlx::query(&self.render(&format!(
                        "SELECT {columns} FROM api_keys ORDER BY created_at DESC"
                    )))
                    .fetch_all(self.pool())
                    .await?
                }
            };
        Ok(rows.iter().map(mappers::row_to_api_key).collect())
    }

    async fn revoke_api_key(&self, id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE api_keys SET disabled = ? WHERE id = ?"))
            .bind(true)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn update_api_key(
        &self,
        id: Uuid,
        name: Option<String>,
        expires_at: Option<Option<DateTime<Utc>>>,
        disabled: Option<bool>,
    ) -> Result<ApiKey, SendableError> {
        let Some(record) = self.fetch_api_key(id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("API key {id} not found"),
            )));
        };
        let mut key = record.key;
        let next_name = name.unwrap_or_else(|| key.name.clone());
        let next_expires_at = expires_at.unwrap_or(key.expires_at);
        let next_disabled = disabled.unwrap_or(key.disabled);
        sqlx::query(
            &self.render("UPDATE api_keys SET name = ?, expires_at = ?, disabled = ? WHERE id = ?"),
        )
        .bind(&next_name)
        .bind(next_expires_at.map(|t| t.timestamp()))
        .bind(next_disabled)
        .bind(id)
        .execute(self.pool())
        .await?;
        key.name = next_name;
        key.expires_at = next_expires_at;
        key.disabled = next_disabled;
        Ok(key)
    }

    async fn touch_api_key(&self, id: Uuid, last_used_at: i64) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE api_keys SET last_used_at = ? WHERE id = ?"))
            .bind(last_used_at)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn create_agent_enrollment_token(
        &self,
        record: AgentEnrollmentTokenRecord,
    ) -> Result<AgentEnrollmentToken, SendableError> {
        let token = record.token;
        let labels = serde_json::to_string(&token.labels)?;
        sqlx::query(&self.render(
            "INSERT INTO agent_enrollment_tokens (token_id, sealed_secret, org_id, labels_json, service_url, spki_pin, permanent, expires_at, consumed_at, issued_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(&token.token_id)
        .bind(record.sealed_secret)
        .bind(token.org_id)
        .bind(labels)
        .bind(&token.service_url)
        .bind(&token.spki_pin)
        .bind(token.permanent)
        .bind(token.expires_at.timestamp())
        .bind(token.consumed_at.map(|value| value.timestamp()))
        .bind(token.issued_by)
        .bind(token.created_at.timestamp())
        .execute(self.pool())
        .await?;
        Ok(token)
    }

    async fn fetch_agent_enrollment_token(
        &self,
        token_id: String,
    ) -> Result<Option<AgentEnrollmentTokenRecord>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT token_id, sealed_secret, org_id, labels_json, service_url, spki_pin, permanent, expires_at, consumed_at, issued_by, created_at \
             FROM agent_enrollment_tokens WHERE token_id = ?",
        ))
        .bind(token_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_agent_enrollment_token_record(&row)))
    }

    async fn list_agent_enrollment_tokens(
        &self,
    ) -> Result<Vec<AgentEnrollmentToken>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT token_id, sealed_secret, org_id, labels_json, service_url, spki_pin, permanent, expires_at, consumed_at, issued_by, created_at \
             FROM agent_enrollment_tokens ORDER BY created_at DESC",
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_agent_enrollment_token_record)
            .map(|record| record.token)
            .collect())
    }

    async fn delete_agent_enrollment_token(&self, token_id: String) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM agent_enrollment_tokens WHERE token_id = ?"))
            .bind(token_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn purge_expired_enrollment_tokens(
        &self,
        before: DateTime<Utc>,
    ) -> Result<u64, SendableError> {
        let result = sqlx::query(&self.render(
            "DELETE FROM agent_enrollment_tokens WHERE expires_at < ? OR consumed_at IS NOT NULL",
        ))
        .bind(before.timestamp())
        .execute(self.pool())
        .await?;
        Ok(result.affected())
    }

    async fn consume_enrollment_token_and_create_api_key(
        &self,
        token_id: String,
        record: ApiKeyRecord,
        consumed_at: DateTime<Utc>,
    ) -> Result<Option<ApiKey>, SendableError> {
        let mut tx = self.pool().begin().await?;
        let consumed = sqlx::query(&self.render(
            "UPDATE agent_enrollment_tokens SET consumed_at = ? \
             WHERE token_id = ? AND consumed_at IS NULL AND expires_at >= ?",
        ))
        .bind(consumed_at.timestamp())
        .bind(token_id)
        .bind(consumed_at.timestamp())
        .execute(&mut *tx)
        .await?;
        if consumed.affected() != 1 {
            tx.rollback().await?;
            return Ok(None);
        }

        let id = record.key.id.unwrap_or_else(Uuid::now_v7);
        sqlx::query(&self.render(
            "INSERT INTO api_keys (id, name, principal_kind, principal_id, system_role, org_id, action_ceiling_json, key_prefix, key_hash, last_used_at, expires_at, disabled, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(&record.key.name)
        .bind(record.key.principal_kind.as_str())
        .bind(record.key.principal_id)
        .bind(record.key.system_role.map(|role| role.as_str().to_string()))
        .bind(record.key.org_id)
        .bind(serde_json::to_string(&record.key.action_ceiling)?)
        .bind(&record.key.key_prefix)
        .bind(&record.key_hash)
        .bind(record.key.last_used_at.map(|value| value.timestamp()))
        .bind(record.key.expires_at.map(|value| value.timestamp()))
        .bind(record.key.disabled)
        .bind(record.key.created_at.timestamp())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut key = record.key;
        key.id = Some(id);
        Ok(Some(key))
    }

    async fn create_session(&self, session: AuthSession) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "INSERT INTO auth_sessions (id, user_id, refresh_token_hash, expires_at, revoked, refresh_count, created_at, last_seen_at, user_agent, ip_address) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(session.id)
        .bind(session.user_id)
        .bind(&session.refresh_token_hash)
        .bind(session.expires_at.timestamp())
        .bind(session.revoked)
        .bind(session.refresh_count)
        .bind(session.created_at.timestamp())
        .bind(session.last_seen_at.timestamp())
        .bind(&session.user_agent)
        .bind(&session.ip_address)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_session_by_hash(
        &self,
        refresh_token_hash: String,
    ) -> Result<Option<AuthSession>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, user_id, refresh_token_hash, expires_at, revoked, refresh_count, created_at, last_seen_at, user_agent, ip_address FROM auth_sessions WHERE refresh_token_hash = ? AND revoked = ?",
        ))
        .bind(refresh_token_hash)
        .bind(false)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_auth_session(&row)))
    }

    async fn consume_session_refresh(
        &self,
        id: Uuid,
        max_refreshes: i64,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE auth_sessions SET revoked = ?, refresh_count = refresh_count + 1 WHERE id = ? AND revoked = ? AND refresh_count < ?",
        ))
        .bind(true)
        .bind(id)
        .bind(false)
        .bind(max_refreshes)
        .execute(self.pool())
        .await?;
        Ok(result.affected() == 1)
    }

    async fn fetch_session(&self, id: Uuid) -> Result<Option<AuthSession>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, user_id, refresh_token_hash, expires_at, revoked, refresh_count, created_at, last_seen_at, user_agent, ip_address FROM auth_sessions WHERE id = ?",
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_auth_session(&row)))
    }

    async fn list_user_sessions(
        &self,
        user_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<AuthSession>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, user_id, refresh_token_hash, expires_at, revoked, refresh_count, created_at, last_seen_at, user_agent, ip_address FROM auth_sessions WHERE user_id = ? AND revoked = ? AND expires_at > ? ORDER BY last_seen_at DESC, created_at DESC",
        ))
        .bind(user_id)
        .bind(false)
        .bind(now.timestamp())
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_auth_session).collect())
    }

    async fn touch_session_activity(
        &self,
        id: Uuid,
        seen_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "UPDATE auth_sessions SET last_seen_at = ?, user_agent = COALESCE(?, user_agent), ip_address = COALESCE(?, ip_address) WHERE id = ? AND revoked = ? AND last_seen_at < ?",
        ))
        .bind(seen_at.timestamp())
        .bind(user_agent)
        .bind(ip_address)
        .bind(id)
        .bind(false)
        .bind(stale_before.timestamp())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn revoke_session(&self, id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE auth_sessions SET revoked = ? WHERE id = ?"))
            .bind(true)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn revoke_user_sessions(&self, user_id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE auth_sessions SET revoked = ? WHERE user_id = ?"))
            .bind(true)
            .bind(user_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn revoke_user_sessions_except(
        &self,
        user_id: Uuid,
        current_session_id: Uuid,
    ) -> Result<(), SendableError> {
        sqlx::query(
            &self.render("UPDATE auth_sessions SET revoked = ? WHERE user_id = ? AND id <> ?"),
        )
        .bind(true)
        .bind(user_id)
        .bind(current_session_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn create_team(&self, name: String, scope: ScopeRef) -> Result<Team, SendableError> {
        if !matches!(scope.kind, ScopeKind::Platform | ScopeKind::Organization) {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "teams must be platform- or organization-scoped",
            )));
        }
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO teams (id, name, scope_kind, scope_id, created_at) VALUES (?, ?, ?, ?, ?)",
        ))
        .bind(id)
        .bind(&name)
        .bind(scope.kind.as_str())
        .bind(scope.id)
        .bind(now)
        .execute(self.pool())
        .await?;
        Ok(Team {
            id: Some(id),
            name,
            scope,
            created_at: DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now),
        })
    }

    async fn update_team(&self, id: Uuid, name: String) -> Result<Team, SendableError> {
        let Some(current) = self.fetch_team(id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Team {id} not found"),
            )));
        };
        sqlx::query(&self.render("UPDATE teams SET name = ? WHERE id = ?"))
            .bind(&name)
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(Team {
            id: Some(id),
            name,
            scope: current.scope,
            created_at: current.created_at,
        })
    }

    async fn fetch_team(&self, id: Uuid) -> Result<Option<Team>, SendableError> {
        let row =
            sqlx::query(&self.render(
                "SELECT id, name, scope_kind, scope_id, created_at FROM teams WHERE id = ?",
            ))
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(mappers::row_to_team))
    }

    async fn list_teams(&self) -> Result<Vec<Team>, SendableError> {
        let rows =
            sqlx::query(&self.render(
                "SELECT id, name, scope_kind, scope_id, created_at FROM teams ORDER BY name",
            ))
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(mappers::row_to_team).collect())
    }

    async fn delete_team(&self, id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            let owned_resources: i64 = sqlx::query_scalar(&self.render(
                "SELECT COUNT(*) FROM resource_ownership WHERE owner_scope_kind = 'team' AND owner_scope_id = ?",
            ))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if owned_resources > 0 {
                return Err(sqlx::Error::Protocol(
                    "transfer resources owned by this team before deleting it".to_string(),
                ));
            }
            sqlx::query(&self.render("DELETE FROM role_assignments WHERE scope_key = ?"))
                .bind(format!("team:{id}"))
                .execute(&mut *tx)
                .await?;
            for sql in [
                "DELETE FROM resource_grants WHERE principal_type = 'team' AND principal_id = ?",
                "DELETE FROM teams WHERE id = ?",
            ] {
                sqlx::query(&self.render(sql))
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn add_team_member(
        &self,
        team_id: Uuid,
        user_id: Uuid,
        role: TeamRole,
    ) -> Result<(), SendableError> {
        let Some(team) = self.fetch_team(team_id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "team not found",
            )));
        };
        let user_exists: i64 = sqlx::query_scalar(
            &self.render("SELECT COUNT(*) FROM users WHERE id = ? AND disabled = ?"),
        )
        .bind(user_id)
        .bind(false)
        .fetch_one(self.pool())
        .await?;
        if user_exists == 0 {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "team member does not exist or is disabled",
            )));
        }
        if team.scope.kind == ScopeKind::Organization {
            let org_id = team.scope.id.expect("organization scope has id");
            let row = sqlx::query(&self.render(
                "SELECT COUNT(*) AS member_count FROM role_assignments WHERE scope_key = ? AND principal_kind = 'user' AND principal_id = ?",
            )).bind(format!("organization:{org_id}")).bind(user_id).fetch_one(self.pool()).await?;
            if row.get::<i64, _>("member_count") == 0 {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "team member must belong to the team's organization",
                )));
            }
        }
        let now = Utc::now().timestamp();
        let conflict = self.dialect().on_conflict_update(
            "principal_kind, principal_id, scope_key",
            &["role", "updated_at"],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO role_assignments (principal_kind, principal_id, scope_key, role, created_by, created_at, updated_at) \
             VALUES ('user', ?, ?, ?, NULL, ?, ?) {conflict}",
        )))
        .bind(user_id).bind(format!("team:{team_id}")).bind(role.as_str())
        .bind(now).bind(now).execute(self.pool()).await?;
        Ok(())
    }

    async fn remove_team_member(&self, team_id: Uuid, user_id: Uuid) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "DELETE FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ? AND scope_key = ?",
        )).bind(user_id).bind(format!("team:{team_id}"))
            .execute(self.pool()).await?;
        Ok(())
    }

    async fn list_user_team_ids(&self, user_id: Uuid) -> Result<Vec<Uuid>, SendableError> {
        let rows = sqlx::query(&self.render("SELECT scope_key FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ? AND scope_key <> 'platform'"))
            .bind(user_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                row.get::<String, _>("scope_key")
                    .strip_prefix("team:")
                    .and_then(|id| Uuid::parse_str(id).ok())
            })
            .collect())
    }

    async fn list_user_teams(&self, user_id: Uuid) -> Result<Vec<Team>, SendableError> {
        let ids = self.list_user_team_ids(user_id).await?;
        let all = self.list_teams().await?;
        Ok(all
            .into_iter()
            .filter(|team| team.id.is_some_and(|id| ids.contains(&id)))
            .collect())
    }

    async fn list_team_members(&self, team_id: Uuid) -> Result<Vec<User>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT u.id, u.username, u.email, u.disabled, u.created_at, u.updated_at \
             FROM users u \
             INNER JOIN role_assignments tm ON tm.principal_id = u.id \
             WHERE tm.principal_kind = 'user' AND tm.scope_key = ? \
             ORDER BY u.username",
        ))
        .bind(format!("team:{team_id}"))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_user).collect())
    }

    async fn create_grant(&self, grant: Grant) -> Result<Grant, SendableError> {
        let id = grant.id.unwrap_or_else(Uuid::now_v7);
        let now = Utc::now().timestamp();
        let conflict = self.dialect().on_conflict_update(
            "resource_type, resource_id, principal_type, principal_id",
            &["permission"],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO resource_grants (id, resource_type, resource_id, principal_type, principal_id, permission, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) {conflict}",
        )))
        .bind(id)
        .bind(grant.resource_type.as_str())
        .bind(grant.resource_id)
        .bind(grant.principal_type.as_str())
        .bind(grant.principal_id)
        .bind(grant.permission.as_str())
        .bind(now)
        .execute(self.pool())
        .await?;
        // read back the canonical row (an upsert keeps the original id).
        let row = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND resource_id = ? AND principal_type = ? AND principal_id = ?",
        ))
        .bind(grant.resource_type.as_str())
        .bind(grant.resource_id)
        .bind(grant.principal_type.as_str())
        .bind(grant.principal_id)
        .fetch_one(self.pool())
        .await?;
        Ok(mappers::row_to_grant(&row))
    }

    async fn revoke_grant(&self, grant_id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            sqlx::query(&self.render("DELETE FROM resource_grants WHERE id = ?"))
                .bind(grant_id)
                .execute(self.pool())
                .await
                .map(|_| ())
        })
        .await?;
        Ok(())
    }

    async fn list_grants(
        &self,
        resource_type: String,
        resource_id: Uuid,
    ) -> Result<Vec<Grant>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND resource_id = ? ORDER BY created_at",
        ))
        .bind(resource_type)
        .bind(resource_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_grant).collect())
    }

    async fn list_user_grants(
        &self,
        resource_type: String,
        user_id: Uuid,
    ) -> Result<Vec<Grant>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND principal_type = 'user' AND principal_id = ?",
        ))
        .bind(resource_type)
        .bind(user_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_grant).collect())
    }

    async fn list_team_grants(
        &self,
        resource_type: String,
        team_id: Uuid,
    ) -> Result<Vec<Grant>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, resource_type, resource_id, principal_type, principal_id, permission, created_at \
             FROM resource_grants WHERE resource_type = ? AND principal_type = 'team' AND principal_id = ?",
        ))
        .bind(resource_type)
        .bind(team_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_grant).collect())
    }
}
