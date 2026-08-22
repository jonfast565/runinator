//! the identity and tenancy http surface: login/tokens/users/API keys/teams, orgs and memberships,
//! and billing plans and invoices.
//!
//! these are the endpoints `AGENTS.md` exempts from the engine-repository rule — thin crud over rows
//! the runtime never orchestrates — which is what lets this crate stand apart from the runtime
//! surface without reaching into `runinator-engine`'s repository layer.

pub mod handlers;

// audit records are written by the engine; this alias keeps the `crate::audit` path the moved
// handler code already uses.
pub(crate) use runinator_engine::audit;
