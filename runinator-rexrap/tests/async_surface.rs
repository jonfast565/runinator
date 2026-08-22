//! the RexRap 1.0 async surface: `async` at the call site, `await`, `detach`, and `task fn`.
//!
//! the property these all defend is that asyncness is a property of the *call site*, never of the
//! callee — one definition serves both schedulings, so nothing is ever written twice.

use runinator_rexrap::{CompileOptions, compile_str, decompile, format_str};

fn options() -> CompileOptions {
    CompileOptions {
        enabled: true,
        ..CompileOptions::default()
    }
}

fn compile(src: &str) -> runinator_models::workflows::WorkflowDefinition {
    compile_str(src, &options()).unwrap_or_else(|err| panic!("compile failed: {err}\n{src}"))
}

#[test]
fn a_plain_call_and_an_async_call_share_one_definition() {
    // the same `task fn` is called both ways; neither call needs a different callee.
    let src = r#"
        task fn deploy(env: string) do {
            let built = console.run(command: "build " ++ env)
        }

        workflow "Both" v1 {
            do {
                deploy(env: "staging")
                let handle = async deploy(env: "prod")
                detach handle
            }
        }
    "#;
    let definition = compile(src);
    let commands: Vec<String> = definition
        .definition
        .nodes
        .iter()
        .filter_map(|node| node.action.as_ref())
        .filter_map(|action| action.configuration.get("command").cloned())
        .map(|value| format!("{value:?}"))
        .collect();
    // both call sites inlined the body, each with its own copy of the substituted argument.
    assert!(
        commands.iter().any(|c| c.contains("staging")),
        "inlined staging call missing: {commands:?}"
    );
    assert!(
        commands.iter().any(|c| c.contains("prod")),
        "inlined prod call missing: {commands:?}"
    );
}

#[test]
fn task_fn_labels_are_namespaced_per_call_site() {
    // two calls to one `task fn` must not collide on the node id its body binds.
    let src = r#"
        task fn step(tag: string) do {
            let work = console.run(command: tag)
        }

        workflow "Twice" v1 {
            do {
                step(tag: "one")
                step(tag: "two")
            }
        }
    "#;
    let definition = compile(src);
    let mut ids: Vec<&str> = definition
        .definition
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate node ids: {ids:?}");
    assert_eq!(
        ids.iter().filter(|id| id.ends_with("__work")).count(),
        2,
        "each call site should carry its own copy of `work`: {ids:?}"
    );
}

#[test]
fn a_pure_fn_may_not_carry_a_runtime_body() {
    let err = compile_str(
        r#"
        fn broken(a: string) do {
            let x = console.run(command: a)
        }

        workflow "Bad" v1 {
            do {
                return 1
            }
        }
    "#,
        &options(),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("task fn"),
        "expected a `task fn` hint, got: {err}"
    );
}

#[test]
fn detach_requires_an_async_binding() {
    let err = compile_str(
        r#"
        workflow "Bad" v1 {
            do {
                let a = console.run(command: "a")
                detach ghost
            }
        }
    "#,
        &options(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("detach"), "got: {err}");
}

#[test]
fn async_requires_a_binding_to_await_later() {
    let err = compile_str(
        r#"
        workflow "Bad" v1 {
            do {
                async console.run(command: "a")
            }
        }
    "#,
        &options(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("async"), "got: {err}");
}

#[test]
fn routes_and_named_joins_round_trip() {
    let src = r#"
        workflow "Routed" v1 {
            do {
                let release = console.run(command: "release")
                routes {
                    on success {
                        continue verify
                    }
                    on failure {
                        continue fail
                    }
                }
            }

            join verify {
                let smoke = console.run(command: "smoke")
                return smoke
            }
        }
    "#;
    let definition = compile(src);
    // the join fronts its region under its own name, so `continue verify` resolves.
    assert!(
        definition
            .definition
            .nodes
            .iter()
            .any(|node| node.id == "verify"),
        "join entry node missing"
    );
    let rendered = decompile(&definition).expect("decompile");
    assert!(rendered.contains("routes {"), "{rendered}");
    assert!(rendered.contains("continue "), "{rendered}");
    // whatever decompile emits must already be in the formatter's canonical shape.
    assert_eq!(
        rendered,
        format_str(&rendered).expect("format"),
        "decompile output is not format-stable"
    );
}

#[test]
fn the_runtime_block_is_mandatory_in_a_workflow() {
    let err = compile_str(
        r#"workflow "Bare" v1 { console.run(command: "a") }"#,
        &options(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("do"), "got: {err}");
}

#[test]
fn two_async_launches_fan_out_into_one_parallel() {
    // the point of `async`: the two launches overlap rather than running one after the other.
    let src = r#"
        workflow "Fanout" v1 {
            do {
                let a = async console.run(command: "a")
                let b = async console.run(command: "b")
                let ra = await a
                let rb = await b
                return ra
            }
        }
    "#;
    let definition = compile(src);
    let parallels = definition
        .definition
        .nodes
        .iter()
        .filter(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Parallel)
        .count();
    assert_eq!(parallels, 1, "expected one fan-out node");
}

#[test]
fn a_lone_async_launch_does_not_manufacture_a_fan_out() {
    // a fan-out of one buys nothing, so it stays an ordinary node.
    let src = r#"
        workflow "Single" v1 {
            do {
                let a = async console.run(command: "a")
                let ra = await a
                return ra
            }
        }
    "#;
    let definition = compile(src);
    assert_eq!(
        definition
            .definition
            .nodes
            .iter()
            .filter(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Parallel)
            .count(),
        0,
        "a single launch should not become a parallel"
    );
}

#[test]
fn work_between_a_launch_and_its_await_overlaps_with_it() {
    // the interleaved statement joins the fan-out as its own branch instead of being pushed
    // behind the launch, which is what keeps it actually concurrent.
    let src = r#"
        workflow "Overlap" v1 {
            do {
                let slow = async console.run(command: "slow")
                let meanwhile = console.run(command: "meanwhile")
                let done = await slow
                return done
            }
        }
    "#;
    let definition = compile(src);
    let parallel = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.kind == runinator_models::workflows::WorkflowNodeKind::Parallel)
        .expect("fan-out node");
    let branches = parallel
        .parameters
        .get("branches")
        .and_then(|value| value.as_array())
        .expect("branches");
    assert_eq!(branches.len(), 2, "launch and interleaved work both branch");
}
