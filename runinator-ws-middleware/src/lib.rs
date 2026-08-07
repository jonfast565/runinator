//! the request-gating layers that sit between the http surface and every handler crate:
//! authentication, authorization, per-principal rate limiting, and global overload protection.
//!
//! it depends on `runinator-ws-core` for the json error envelope it replies with, and on nothing
//! that registers a route — `runinator-ws` applies these layers when it assembles the router.

pub mod auth;
pub mod authz;
pub mod overload;
pub mod rate_limit;

// audit records and workflow/pipeline lookups used by the resource-grant checks live in the engine;
// these aliases keep the `crate::audit`/`crate::repository` paths the moved code already uses.
pub(crate) use runinator_engine::{audit, repository};

pub use auth::{AuthConfig, AuthOptions, AuthState, auth_middleware};
pub use overload::{OverloadConfig, apply_overload_protection};
pub use rate_limit::{RateLimitConfig, RateLimiter, rate_limit_middleware};
