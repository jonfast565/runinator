use runinator_auth::AuthConfig;
use runinator_models::auth::ReplicaClaims;

// env vars configuring the broker's bearer-token gate. when no secret is set the broker stays open
// (the local/dev default); the supervisor stack is unaffected.
const SECRET_ENV: &str = "RUNINATOR_BROKER_AUTH_SECRET";
const SECRET_PREVIOUS_ENV: &str = "RUNINATOR_BROKER_AUTH_SECRET_PREVIOUS";

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;

mod broker_auth;
pub use broker_auth::BrokerAuth;

mod auth_identity;
pub use auth_identity::AuthIdentity;
