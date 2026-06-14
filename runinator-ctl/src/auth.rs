use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use runinator_api::{ApiError, AsyncApiClient, StaticLocator};
use runinator_models::auth::LoginResponse;
use runinator_models::json;
use runinator_utilities::app_data::{app_data_dir, app_data_path};
use serde::{Deserialize, Serialize};

use crate::{
    cli::Cli,
    commands::{self, Result},
    output,
};

const SESSION_FILE: &str = "ctl-session.json";

type Client = AsyncApiClient<StaticLocator>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    api_base_url: String,
    username: String,
    access_token: String,
    refresh_token: String,
}

pub async fn login(cli: &Cli, username: Option<String>, password: Option<String>) -> Result<()> {
    let username = username.unwrap_or(prompt("username")?);
    let password = password.unwrap_or(prompt("password")?);
    let client = AsyncApiClient::new(StaticLocator::new(cli.api_base_url.clone()))?;
    let session = client.login(&username, &password).await?;
    let stored = StoredSession {
        api_base_url: cli.api_base_url.clone(),
        username: session.user.username.clone(),
        access_token: session.access_token,
        refresh_token: session.refresh_token,
    };
    write_session(&stored)?;
    if cli.json {
        return output::json(&json!({
            "logged_in": true,
            "api_base_url": stored.api_base_url,
            "username": stored.username,
        }));
    }
    println!(
        "logged in to {} as {}.",
        stored.api_base_url, stored.username
    );
    Ok(())
}

pub async fn logout(cli: &Cli) -> Result<()> {
    let Some(stored) = read_session()? else {
        if cli.json {
            return output::json(&json!({ "logged_out": false, "reason": "not_logged_in" }));
        }
        println!("not logged in.");
        return Ok(());
    };

    if !same_api_base(&stored.api_base_url, &cli.api_base_url) {
        if cli.json {
            return output::json(&json!({
                "logged_out": false,
                "reason": "session_is_for_different_api_base_url",
                "api_base_url": stored.api_base_url,
            }));
        }
        println!("stored session belongs to {}.", stored.api_base_url);
        return Ok(());
    }

    let login = refresh_with_client(&cli.api_base_url, &stored.refresh_token).await;
    if let Ok(refreshed) = login {
        let client = AsyncApiClient::with_credentials(
            StaticLocator::new(cli.api_base_url.clone()),
            Some(refreshed.access_token),
        )?;
        let _ = client.logout(&refreshed.refresh_token).await;
    }

    remove_session_file()?;
    if cli.json {
        return output::json(&json!({
            "logged_out": true,
            "api_base_url": cli.api_base_url,
        }));
    }
    println!("logged out from {}.", cli.api_base_url);
    Ok(())
}

pub async fn build_authenticated_client(cli: &Cli) -> Result<Client> {
    if let Some(api_key) = cli.api_key.clone().filter(|value| !value.trim().is_empty()) {
        return Ok(AsyncApiClient::with_credentials(
            StaticLocator::new(cli.api_base_url.clone()),
            Some(api_key),
        )?);
    }

    let unauthenticated = AsyncApiClient::new(StaticLocator::new(cli.api_base_url.clone()))?;
    let auth = unauthenticated.fetch_auth_config().await?;
    if !auth.enabled {
        return Ok(unauthenticated);
    }

    let Some(stored) = read_session()? else {
        return Err(commands::err(login_required_message(&cli.api_base_url)));
    };
    if !same_api_base(&stored.api_base_url, &cli.api_base_url) {
        return Err(commands::err(login_required_message(&cli.api_base_url)));
    }

    let refreshed = match unauthenticated.refresh_session(&stored.refresh_token).await {
        Ok(session) => session,
        Err(err) if should_forget_session(&err) => {
            remove_session_file()?;
            return Err(commands::err(login_required_message(&cli.api_base_url)));
        }
        Err(err) => return Err(Box::new(err)),
    };
    let stored = StoredSession {
        api_base_url: cli.api_base_url.clone(),
        username: refreshed.user.username.clone(),
        access_token: refreshed.access_token.clone(),
        refresh_token: refreshed.refresh_token.clone(),
    };
    write_session(&stored)?;
    Ok(AsyncApiClient::with_credentials(
        StaticLocator::new(cli.api_base_url.clone()),
        Some(refreshed.access_token),
    )?)
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let value = input.trim().to_owned();
    if value.is_empty() {
        return Err(commands::err(format!("{label} is required")));
    }
    Ok(value)
}

fn login_required_message(api_base_url: &str) -> String {
    format!(
        "the server at {api_base_url} requires authentication; run `runinatorctl --api-base-url {api_base_url} login` first"
    )
}

async fn refresh_with_client(api_base_url: &str, refresh_token: &str) -> Result<LoginResponse> {
    let client = AsyncApiClient::new(StaticLocator::new(api_base_url.to_owned()))?;
    Ok(client.refresh_session(refresh_token).await?)
}

fn should_forget_session(err: &ApiError) -> bool {
    matches!(err, ApiError::Http { status, .. } if status.as_u16() == 401 || status.as_u16() == 403)
}

fn same_api_base(left: &str, right: &str) -> bool {
    left.trim_end_matches('/') == right.trim_end_matches('/')
}

fn read_session() -> Result<Option<StoredSession>> {
    let path = session_path()?;
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

fn write_session(session: &StoredSession) -> Result<()> {
    fs::create_dir_all(app_data_dir()?)?;
    let path = session_path()?;
    fs::write(&path, serde_json::to_vec_pretty(session)?)?;
    set_private_permissions(&path)?;
    Ok(())
}

fn remove_session_file() -> Result<()> {
    let path = session_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn session_path() -> Result<PathBuf> {
    Ok(app_data_path(SESSION_FILE)?)
}

#[cfg(unix)]
fn set_private_permissions(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &PathBuf) -> Result<()> {
    Ok(())
}
