//! Mission inputs identify both the requested work and its immutable source checkout.

use super::*;

#[test]
fn mission_input_requires_goal_repository_and_revision() {
    let valid = runinator_models::json!({
        "request": { "goal": "fix the loop" },
        "mission": {
            "source": {
                "repository": "https://example.invalid/repo.git",
                "revision": "0123456789abcdef"
            }
        }
    });
    let payload = valid.as_object().unwrap();
    let mission = payload.get("mission").and_then(Value::as_object).unwrap();
    validate_mission_input(payload, mission).unwrap();

    for pointer in [
        "/request/goal",
        "/mission/source/repository",
        "/mission/source/revision",
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = Value::String(String::new());
        let payload = invalid.as_object().unwrap();
        let mission = payload.get("mission").and_then(Value::as_object).unwrap();
        assert!(
            validate_mission_input(payload, mission).is_err(),
            "{pointer}"
        );
    }
}
