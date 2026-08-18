//! covers the consecutive-failure budget: when it is spent, and what clears it.

use super::*;

#[test]
fn an_unlimited_budget_is_never_spent() {
    let budget = ReconnectBudget::new(None);
    for _ in 0..1_000 {
        assert!(!budget.charge("broker down").spent);
    }
    assert!(!budget.is_spent());
}

// `0` is the "retry forever" spelling a host config carries, so it must not mean "give up at once".
#[test]
fn a_zero_budget_is_treated_as_unlimited() {
    let budget = ReconnectBudget::new(Some(0));
    assert_eq!(budget.max_attempts(), None);
    assert!(!budget.charge("broker down").spent);
}

#[test]
fn the_budget_is_spent_on_its_last_attempt() {
    let budget = ReconnectBudget::new(Some(3));
    assert_eq!(
        budget.charge("one"),
        Charge {
            attempt: 1,
            spent: false
        }
    );
    assert_eq!(
        budget.charge("two"),
        Charge {
            attempt: 2,
            spent: false
        }
    );
    assert_eq!(
        budget.charge("three"),
        Charge {
            attempt: 3,
            spent: true
        }
    );
    assert!(budget.is_spent());
    assert_eq!(budget.reason(), "three");
}

// the count bounds one outage, not the machine's lifetime: a connection that worked clears it.
#[test]
fn a_successful_connection_clears_the_count() {
    let budget = ReconnectBudget::new(Some(3));
    budget.charge("one");
    budget.charge("two");
    budget.reset();
    assert_eq!(budget.attempts(), 0);
    assert!(!budget.charge("three").spent);
}

// giving up is terminal: the transport can still publish a reconnect after the agent decided to
// stop, and that must not un-spend the budget and leave the lifecycle running.
#[test]
fn a_reset_after_giving_up_does_not_revive_the_budget() {
    let budget = ReconnectBudget::new(Some(1));
    assert!(budget.charge("gone").spent);
    budget.reset();
    assert!(budget.is_spent());
    assert_eq!(budget.attempts(), 1);
}

#[tokio::test]
async fn waiting_on_an_already_spent_budget_returns_immediately() {
    let budget = ReconnectBudget::new(Some(1));
    budget.charge("gone");
    budget.wait_spent().await;
}

#[tokio::test]
async fn waiting_resolves_when_another_task_spends_the_budget() {
    let budget = std::sync::Arc::new(ReconnectBudget::new(Some(2)));
    let charger = std::sync::Arc::clone(&budget);
    tokio::spawn(async move {
        charger.charge("one");
        charger.charge("two");
    });
    budget.wait_spent().await;
    assert_eq!(budget.attempts(), 2);
}
