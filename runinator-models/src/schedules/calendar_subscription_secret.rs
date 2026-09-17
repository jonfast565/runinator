#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSubscriptionSecret {
    pub subscription: CalendarSubscription,
    pub token: String,
}
