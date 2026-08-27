use std::time::Duration;

use runinator_models::errors::SendableError;

use crate::errors::STATEMENT_TIMEOUT;

/// Wrap a database future in the statement timeout, mapping the elapsed case to `DB006`.
pub(crate) async fn with_timeout<F, T>(future: F, timeout: Duration) -> Result<T, SendableError>
where
    F: std::future::Future<Output = Result<T, SendableError>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => Err(STATEMENT_TIMEOUT.error(format!(
            "statement timed out after {} seconds",
            timeout.as_secs()
        ))),
    }
}
