//! argument aliases and object spread: that they lower identically to the explicit form, and
//! that decompiling resugars them.

use super::*;

#[test]
fn alias_spread_lowers_like_explicit_args() {
    let aliased = r#"
        workflow "Aliased" v1 {
            alias conn = { base_url: config.jira.base_url, token: secret.jira.token }
            node t <- jira.transition(...conn, key: "ABC-1")
        }
    "#;
    let explicit = r#"
        workflow "Aliased" v1 {
            node t <- jira.transition(base_url: config.jira.base_url, token: secret.jira.token, key: "ABC-1")
        }
    "#;
    assert_eq!(
        runtime_graph(compile(aliased)),
        runtime_graph(compile(explicit)),
        "a `...alias` spread should lower identically to the explicit argument list"
    );
}
#[test]
fn explicit_arg_overrides_spread() {
    // the explicit `base_url` wins over the alias's `base_url` regardless of source order.
    let aliased = r#"
        workflow "Override" v1 {
            alias conn = { base_url: "from-alias", region: "us" }
            node t <- api.call(...conn, base_url: "explicit")
        }
    "#;
    let explicit = r#"
        workflow "Override" v1 {
            node t <- api.call(base_url: "explicit", region: "us")
        }
    "#;
    assert_eq!(
        runtime_graph(compile(aliased)),
        runtime_graph(compile(explicit))
    );
}
#[test]
fn unknown_alias_spread_is_a_semantic_error() {
    let src = r#"
        workflow "Bad" v1 {
            node t <- api.call(...missing, key: "x")
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("unknown alias"), "{message}");
}
#[test]
fn duplicate_alias_is_a_semantic_error() {
    let src = r#"
        workflow "Dup" v1 {
            alias conn = { a: "1" }
            alias conn = { b: "2" }
            node t <- api.call(...conn)
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("duplicate alias"), "{message}");
}
#[test]
fn format_preserves_alias_and_spread() {
    let src = r#"
        workflow "Fmt" v1 {
            alias conn = { base_url: config.jira.base_url, token: secret.jira.token }
            node t <- jira.transition(...conn, key: "ABC-1")
        }
    "#;
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("alias conn = {"), "{formatted}");
    assert!(formatted.contains("...conn"), "{formatted}");
    // formatting is idempotent and never expands the sugar.
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
}

// normalize a definition's graph and drop the render-only `rexrap` metadata sidecar (declared
// types, alias declarations, spread recipes), so forms that differ only in resugar hints —
// aliased vs. fully-expanded source — compare equal on their runtime graph.
fn runtime_graph(definition: runinator_models::workflows::WorkflowDefinition) -> Value {
    let mut value = runinator_workflows::normalize_definition(definition.definition).as_value();
    if let Value::Object(root) = &mut value {
        root.remove("metadata");
    }
    value
}

// helper: compile two sources and assert their runtime graphs match (ignoring resugar hints).
fn assert_same_graph(aliased: &str, explicit: &str) {
    assert_eq!(
        runtime_graph(compile(aliased)),
        runtime_graph(compile(explicit)),
        "aliased form should lower identically to the explicit form"
    );
}
#[test]
fn object_spread_in_subflow_with_matches_explicit() {
    assert_same_graph(
        r#"
        workflow "Sub" v1 {
            alias conn = { base_url: config.a.b, token: secret.c.d }
            subflow("Child", params: { ...conn, key: "K" })
        }
        "#,
        r#"
        workflow "Sub" v1 {
            subflow("Child", params: { base_url: config.a.b, token: secret.c.d, key: "K" })
        }
        "#,
    );
}
#[test]
fn object_spread_in_approval_metadata_matches_explicit() {
    assert_same_graph(
        r#"
        workflow "Appr" v1 {
            alias meta = { env: "prod", owner: "team" }
            approve "Ship?" type "change" { ...meta, extra: "x" }
                ok -> done
                reject -> fail
        }
        "#,
        r#"
        workflow "Appr" v1 {
            approve "Ship?" type "change" { env: "prod", owner: "team", extra: "x" }
                ok -> done
                reject -> fail
        }
        "#,
    );
}
#[test]
fn nested_object_spread_inside_action_arg() {
    assert_same_graph(
        r#"
        workflow "Nest" v1 {
            alias conn = { base_url: config.a.b }
            node t <- api.call(config: { ...conn, timeout: 30 })
        }
        "#,
        r#"
        workflow "Nest" v1 {
            node t <- api.call(config: { base_url: config.a.b, timeout: 30 })
        }
        "#,
    );
}
#[test]
fn aliases_compose_via_spread() {
    assert_same_graph(
        r#"
        workflow "Compose" v1 {
            alias base = { base_url: config.a.b }
            alias full = { ...base, token: secret.c.d }
            node t <- api.call(...full)
        }
        "#,
        r#"
        workflow "Compose" v1 {
            node t <- api.call(base_url: config.a.b, token: secret.c.d)
        }
        "#,
    );
}
#[test]
fn alias_cycle_is_a_semantic_error() {
    let src = r#"
        workflow "Cycle" v1 {
            alias a = { ...b }
            alias b = { ...a }
            node t <- api.call(...a)
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("references itself"), "{message}");
}
#[test]
fn later_entry_overrides_spread() {
    // `(x: "from-arg", ...conn)` — the spread is last, so conn's x wins (positional last-wins).
    assert_same_graph(
        r#"
        workflow "Last" v1 {
            alias conn = { x: "from-alias" }
            node t <- api.call(x: "from-arg", ...conn)
        }
        "#,
        r#"
        workflow "Last" v1 {
            node t <- api.call(x: "from-alias")
        }
        "#,
    );
}

// compile -> decompile -> recompile and assert the full normalized graphs (including the `rexrap`
// resugar sidecar) match, so the alias declarations and `...alias` spreads round-trip exactly.
// returns the decompiled source for spot-checks on the recovered surface syntax.
fn assert_alias_round_trips(src: &str) -> String {
    let first = compile(src);
    let rexrap = decompile(&first).expect("decompile");
    let second = compile_str(&rexrap, &default_test_options())
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- decompiled ---\n{rexrap}"));
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition),
        runinator_workflows::normalize_definition(second.definition),
        "alias round trip diverged\n--- decompiled ---\n{rexrap}"
    );
    rexrap
}
#[test]
fn resugars_action_spread() {
    let rexrap = assert_alias_round_trips(
        r#"
        workflow "Act" v1 {
            alias conn = { base_url: config.jira.base_url, token: secret.jira.token }
            node t <- jira.transition(...conn, key: "ABC-1")
        }
        "#,
    );
    assert!(rexrap.contains("alias conn = {"), "{rexrap}");
    assert!(rexrap.contains("...conn"), "{rexrap}");
    assert!(rexrap.contains(r#"key: "ABC-1""#), "{rexrap}");
}
#[test]
fn resugars_subflow_with_spread() {
    let rexrap = assert_alias_round_trips(
        r#"
        workflow "Sub" v1 {
            alias conn = { base_url: config.a.b, token: secret.c.d }
            subflow("Child", params: { ...conn, key: "K" })
        }
        "#,
    );
    assert!(rexrap.contains("alias conn = {"), "{rexrap}");
    assert!(rexrap.contains("params: {"), "{rexrap}");
    assert!(rexrap.contains("...conn"), "{rexrap}");
}
#[test]
fn resugars_approval_metadata_spread() {
    let rexrap = assert_alias_round_trips(
        r#"
        workflow "Appr" v1 {
            alias meta = { env: "prod", owner: "team" }
            approve "Ship?" type "change" { ...meta, extra: "x" }
                ok -> done
                reject -> fail
        }
        "#,
    );
    assert!(rexrap.contains("alias meta = {"), "{rexrap}");
    assert!(rexrap.contains("...meta"), "{rexrap}");
}
#[test]
fn resugars_nested_object_spread() {
    let rexrap = assert_alias_round_trips(
        r#"
        workflow "Nest" v1 {
            alias conn = { base_url: config.a.b }
            node t <- api.call(config: { ...conn, timeout: 30 })
        }
        "#,
    );
    // the nested object keeps its `...conn` spread; the formatter lays it out one entry per line.
    assert!(rexrap.contains("config: {"), "{rexrap}");
    assert!(rexrap.contains("...conn"), "{rexrap}");
}
#[test]
fn resugars_alias_composition() {
    let rexrap = assert_alias_round_trips(
        r#"
        workflow "Compose" v1 {
            alias base = { base_url: config.a.b }
            alias full = { ...base, token: secret.c.d }
            node t <- api.call(...full)
        }
        "#,
    );
    // the composing alias keeps its `...base` spread in the recovered header (one entry per line).
    assert!(rexrap.contains("alias full = {"), "{rexrap}");
    assert!(rexrap.contains("...base"), "{rexrap}");
    assert!(rexrap.contains("...full"), "{rexrap}");
}
#[test]
fn resugars_override_keeping_authored_order() {
    // spread-first: the explicit override stays after the spread.
    let first = assert_alias_round_trips(
        r#"
        workflow "Over" v1 {
            alias conn = { base_url: "from-alias", region: "us" }
            node t <- api.call(...conn, base_url: "explicit")
        }
        "#,
    );
    // arguments now lay out one per line; the override still follows the spread in source order.
    assert!(
        ordered(&first, "...conn", r#"base_url: "explicit""#),
        "{first}"
    );

    // spread-last: the explicit entry stays before the spread (which wins on recompile).
    let second = assert_alias_round_trips(
        r#"
        workflow "Over2" v1 {
            alias conn = { x: "from-alias" }
            node t <- api.call(x: "from-arg", ...conn)
        }
        "#,
    );
    assert!(ordered(&second, r#"x: "from-arg""#, "...conn"), "{second}");
}

// parameter defaults --------------------------------------------------------
