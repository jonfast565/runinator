use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use runinator_api::{ApiError, AsyncApiClient, StaticLocator};
use runinator_models::auth::LoginResponse;
use runinator_models::json;
use runinator_platform::app_data::{app_data_dir, app_data_path};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    commands::{self, Result},
    output,
};
use runinator_ctl_core::cli::Cli;

const SESSION_FILE: &str = "ctl-session.json";

type Client = AsyncApiClient<StaticLocator>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    api_base_url: String,
    username: String,
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    active_org_id: Option<Uuid>,
}

pub async fn login(cli: &Cli) -> Result<()> {
    let username = credential(&cli.username).map_or_else(|| prompt("username"), Ok)?;
    let password = credential(&cli.password).map_or_else(|| prompt("password"), Ok)?;
    let stored = store_login(cli, &username, &password).await?;
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

/// exchange credentials for a session and persist it for later commands.
async fn store_login(cli: &Cli, username: &str, password: &str) -> Result<StoredSession> {
    let client = AsyncApiClient::new(StaticLocator::new(cli.api_base_url.clone()))?;
    let session = client.login(username, password).await?;
    let stored = StoredSession {
        api_base_url: cli.api_base_url.clone(),
        username: session.user.username.clone(),
        access_token: session.access_token,
        refresh_token: session.refresh_token,
        active_org_id: None,
    };
    write_session(&stored)?;
    Ok(stored)
}

/// non-empty credential supplied on the command line or through the environment.
fn credential(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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

/// an authenticated client, or an unauthenticated one when the web service cannot be reached.
///
/// for the MCP server only. it is started by its client, often before the web service is up, and a
/// process that exits at startup is one the client marks failed for the whole session. an
/// unreachable service becomes an error on the first tool call instead — which names the real
/// problem, to a caller that can retry it.
pub async fn build_client_or_offline(cli: &Cli) -> Result<Client> {
    match build_authenticated_client(cli).await {
        Ok(client) => Ok(client),
        Err(_) => Ok(AsyncApiClient::new(StaticLocator::new(
            cli.api_base_url.clone(),
        ))?),
    }
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

    // credentials passed on the command line (or through the environment) log in on demand, so a
    // one-shot command and the repl both work without a separate `login` first.
    let credentials = credential(&cli.username).zip(credential(&cli.password));
    let usable_session = read_session()?.filter(|stored| {
        same_api_base(&stored.api_base_url, &cli.api_base_url)
            && credentials
                .as_ref()
                .is_none_or(|(username, _)| &stored.username == username)
    });

    let Some(stored) = usable_session else {
        return match credentials {
            Some((username, password)) => {
                let stored = store_login(cli, &username, &password).await?;
                Ok(AsyncApiClient::with_credentials(
                    StaticLocator::new(cli.api_base_url.clone()),
                    Some(stored.access_token),
                )?)
            }
            None => Err(commands::err(login_required_message(&cli.api_base_url))),
        };
    };

    let refreshed = match unauthenticated.refresh_session(&stored.refresh_token).await {
        Ok(session) => session,
        Err(err) if should_forget_session(&err) => {
            remove_session_file()?;
            // an expired session is recoverable when credentials are at hand.
            let Some((username, password)) = credentials else {
                return Err(commands::err(login_required_message(&cli.api_base_url)));
            };
            let stored = store_login(cli, &username, &password).await?;
            return Ok(AsyncApiClient::with_credentials(
                StaticLocator::new(cli.api_base_url.clone()),
                Some(stored.access_token),
            )?);
        }
        Err(err) => return Err(Box::new(err)),
    };
    // Persist the rotation before restoring the selected scope. If the membership was revoked
    // between invocations, the new refresh token remains usable and the stale selection is
    // cleared instead of stranding the user behind a consumed refresh token.
    let mut refreshed_stored = StoredSession {
        api_base_url: cli.api_base_url.clone(),
        username: refreshed.user.username.clone(),
        access_token: refreshed.access_token.clone(),
        refresh_token: refreshed.refresh_token.clone(),
        active_org_id: stored.active_org_id,
    };
    write_session(&refreshed_stored)?;

    let mut access_token = refreshed.access_token;
    if let Some(org_id) = refreshed_stored.active_org_id {
        let scoped = AsyncApiClient::with_credentials(
            StaticLocator::new(cli.api_base_url.clone()),
            Some(access_token.clone()),
        )?;
        access_token = match scoped.switch_org(org_id).await {
            Ok(context) => context.access_token,
            Err(error) => {
                refreshed_stored.active_org_id = None;
                write_session(&refreshed_stored)?;
                return Err(Box::new(error));
            }
        };
    }
    refreshed_stored.access_token = access_token.clone();
    write_session(&refreshed_stored)?;
    Ok(AsyncApiClient::with_credentials(
        StaticLocator::new(cli.api_base_url.clone()),
        Some(access_token),
    )?)
}

/// Persist the access token and active scope selected by an `orgs use` or `orgs platform` command.
pub(crate) fn persist_active_scope(
    api_base_url: &str,
    access_token: String,
    active_org_id: Option<Uuid>,
) -> Result<()> {
    let Some(mut stored) = read_session()? else {
        return Err(commands::err("no stored session is available to update"));
    };
    if !same_api_base(&stored.api_base_url, api_base_url) {
        return Err(commands::err(
            "stored session belongs to a different API base URL",
        ));
    }
    stored.access_token = access_token;
    stored.active_org_id = active_org_id;
    write_session(&stored)
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
        "the server at {api_base_url} requires authentication; run `runinatorctl --api-base-url {api_base_url} login` first, or pass --username with RUNINATOR_PASSWORD set"
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
    app_data_path(SESSION_FILE)
}

#[cfg(unix)]
fn set_private_permissions(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

// windows has no posix mode bits; restricting the session file to the current user would require
// acl apis (e.g. SetNamedSecurityInfo), which isn't worth a new dependency for this one call site.
#[cfg(not(unix))]
fn set_private_permissions(_path: &PathBuf) -> Result<()> {
    Ok(())
}
