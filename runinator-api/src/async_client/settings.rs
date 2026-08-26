use chrono::{DateTime, Utc};
use runinator_models::{
    api_routes::API_CREDENTIALS,
    json,
    settings::{SettingKind, SettingSummary},
    value::Value,
};

use crate::{locator::ServiceLocator, ApiError, Result};

use super::AsyncApiClient;

impl<L> AsyncApiClient<L>
where
    L: ServiceLocator,
{
    pub async fn list_settings(&self) -> Result<Vec<SettingSummary>> {
        let url = self.build_url(API_CREDENTIALS).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<SettingSummary>>().await?)
    }

    pub async fn get_setting(&self, kind: SettingKind, scope: &str, name: &str) -> Result<Value> {
        let mut url = self.build_url(API_CREDENTIALS).await?;
        url.query_pairs_mut()
            .append_pair("kind", kind.as_str())
            .append_pair("scope", scope)
            .append_pair("name", name);
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        body.get("value")
            .cloned()
            .ok_or_else(|| ApiError::UnexpectedResponse("missing setting value".into()))
    }

    pub async fn put_setting(
        &self,
        kind: SettingKind,
        scope: &str,
        name: &str,
        value: &Value,
        schema: Option<&Value>,
    ) -> Result<Value> {
        self.put_setting_with_expiry(kind, scope, name, value, schema, None)
            .await
    }

    pub async fn put_setting_with_expiry(
        &self,
        kind: SettingKind,
        scope: &str,
        name: &str,
        value: &Value,
        schema: Option<&Value>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Value> {
        let url = self.build_url(API_CREDENTIALS).await?;
        let mut body = json!({
            "scope": scope,
            "name": name,
            "value": value,
            "kind": kind.as_str(),
        });
        if let (Some(schema), Some(object)) = (schema, body.as_object_mut()) {
            object.insert("schema".into(), schema.clone());
        }
        if let (Some(expires_at), Some(object)) = (expires_at, body.as_object_mut()) {
            object.insert("expires_at".into(), Value::String(expires_at.to_rfc3339()));
        }
        let response = self.http_post(url.clone()).json(&body).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn delete_setting(
        &self,
        kind: SettingKind,
        scope: &str,
        name: &str,
    ) -> Result<Value> {
        let mut url = self.build_url(API_CREDENTIALS).await?;
        url.query_pairs_mut()
            .append_pair("kind", kind.as_str())
            .append_pair("scope", scope)
            .append_pair("name", name);
        let response = self.http_delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn move_setting(
        &self,
        id: uuid::Uuid,
        kind: SettingKind,
        scope: &str,
        name: &str,
    ) -> Result<Value> {
        let url = self.build_url(&format!("/credentials/{id}")).await?;
        let response = self
            .http_patch(url.clone())
            .json(&json!({ "kind": kind, "scope": scope, "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }
}
