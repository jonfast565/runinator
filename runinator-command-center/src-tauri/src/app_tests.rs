//! command registry parity tests.

use std::collections::BTreeSet;

fn http_command_names() -> BTreeSet<&'static str> {
    let source = include_str!("../../src/core/api/httpRuntime.ts");
    let registry = source
        .split_once("const REGISTRY")
        .map(|(_, registry)| registry)
        .expect("http command registry");

    registry
        .lines()
        .take_while(|line| *line != "};")
        .filter_map(|line| {
            let entry = line.strip_prefix("  ")?;
            if entry.starts_with(' ') {
                return None;
            }
            let (name, suffix) = entry.split_once(':')?;
            suffix.trim_start().starts_with('{').then_some(name)
        })
        .collect()
}

fn tauri_command_names() -> BTreeSet<&'static str> {
    include_str!("app.rs")
        .lines()
        .filter_map(|line| {
            let (_, command) = line.trim().strip_suffix(',')?.rsplit_once("::")?;
            command
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
                .then_some(command)
        })
        .collect()
}

#[test]
fn every_http_command_has_a_tauri_bridge() {
    let http = http_command_names();
    let tauri = tauri_command_names();
    let missing = http.difference(&tauri).copied().collect::<Vec<_>>();

    assert!(missing.is_empty(), "missing Tauri commands: {missing:?}");
}
