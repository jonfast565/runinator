use runinator_models::{
    api_routes::{API_AUTH_CONFIG, API_AUTH_LOGIN, API_AUTH_LOGOUT, API_AUTH_REFRESH},
    auth::{AuthConfigResponse, LoginRequest, LoginResponse, RefreshRequest},
    web::TaskResponse,
};

use crate::{locator::ServiceLocator, Result};

use super::AsyncApiClient;

impl<L> AsyncApiClient<L>
where
    L: ServiceLocator,
{
    pub async fn fetch_auth_config(&self) -> Result<AuthConfigResponse> {
        let url = self.build_url(API_AUTH_CONFIG).await?;
        let response = self.http_get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<AuthConfigResponse>().await?)
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<LoginResponse> {
        let url = self.build_url(API_AUTH_LOGIN).await?;
        let response = self
            .http_post(url.clone())
            .json(&LoginRequest {
                username: username.to_owned(),
                password: password.to_owned(),
            })
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<LoginResponse>().await?)
    }

    pub async fn refresh_session(&self, refresh_token: &str) -> Result<LoginResponse> {
        let url = self.build_url(API_AUTH_REFRESH).await?;
        let response = self
            .http_post(url.clone())
            .json(&RefreshRequest {
                refresh_token: refresh_token.to_owned(),
            })
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<LoginResponse>().await?)
    }

    pub async fn logout(&self, refresh_token: &str) -> Result<TaskResponse> {
        let url = self.build_url(API_AUTH_LOGOUT).await?;
        let response = self
            .http_post(url.clone())
            .json(&RefreshRequest {
                refresh_token: refresh_token.to_owned(),
            })
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }
}
