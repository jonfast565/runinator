#[allow(unused_imports)]
use super::*;

pub trait AuthorizationStore:
    AuthStore + RbacStore + ScheduleStore + RuntimeStore + AutomationStore
{
}

impl<T> AuthorizationStore for T where
    T: AuthStore + RbacStore + ScheduleStore + RuntimeStore + AutomationStore
{
}
