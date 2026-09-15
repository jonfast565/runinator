//! bounded remote-log formatting helpers.

use super::*;

#[test]
fn formatted_levels_map_to_the_shared_severity_names() {
    assert_eq!(
        level_and_target("2026-01-01 ERROR worker: failed", "runtime"),
        ("error", "worker".to_string())
    );
    assert_eq!(
        level_and_target("2026-01-01 WARN worker: slow", "runtime"),
        ("warn", "worker".to_string())
    );
    assert_eq!(
        level_and_target("ordinary event", "runtime"),
        ("info", "runtime".to_string())
    );
}
