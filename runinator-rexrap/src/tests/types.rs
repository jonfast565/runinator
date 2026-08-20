//! the type system at the surface: named type declarations, provider-inferred result types,
//! constrained types, casts, and closures.

use super::*;

#[test]
fn round_trips_named_type_decls() {
    let src = r#"
        workflow "Typed" v1 {
            params {
                cart: Cart
            }
            type Cart { subtotal: number, tax: number }
            type Ids = integer[]
            console.run(command: "go")
        }
    "#;
    assert_round_trips(src);
    let rexrap = decompile(&compile(src)).expect("decompile");
    assert!(
        rexrap.contains("type Cart {"),
        "struct decl missing:\n{rexrap}"
    );
    assert!(
        rexrap.contains("type Ids = integer[]"),
        "alias decl missing:\n{rexrap}"
    );
    // the parameter field references the declared name, not the expanded struct shape.
    assert!(
        rexrap.contains("cart: Cart"),
        "named parameter ref missing:\n{rexrap}"
    );
    // a struct type renders each field on its own indented line, not collapsed inline.
    assert!(
        rexrap.contains("type Cart {\n        subtotal: number\n        tax: number\n    }"),
        "struct decl not rendered multiline:\n{rexrap}"
    );
}
#[test]
fn round_trips_named_type_decls_with_aliases() {
    let src = r#"
        workflow "Typed" v1 {
            type Payload = { response: any }
            alias shared = { input: "hello" }
            node probe: Payload <- ai-command.execute(command: "echo", ...shared)
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn named_type_preserved_on_let_annotation() {
    let src = r#"
        workflow "Typed" v1 {
            type Cart { subtotal: number, tax: number }
            node probe: Cart <- console.run(command: "probe")
            console.run(command: "after")
        }
    "#;
    assert_round_trips(src);
    let rexrap = decompile(&compile(src)).expect("decompile");
    // the node annotation keeps the declared name rather than expanding the struct.
    assert!(
        rexrap.contains("node probe: Cart"),
        "named node ref missing:\n{rexrap}"
    );
}
#[test]
fn named_type_resolves_in_input() {
    let src = r#"
        workflow "Typed" v1 {
            params { cart: Cart }
            type Cart { subtotal: number, tax: number }
            console.run(command: "go")
        }
    "#;
    let definition = compile(src);
    // the input `cart` field resolves to the declared closed struct, not Any.
    let cart = definition.input_type.field("cart").expect("cart field");
    assert!(matches!(cart, RuninatorType::Struct { .. }));
}
#[test]
fn rejects_cyclic_type_decls() {
    let src = r#"
        workflow "Cycle" v1 {
            type A = B
            type B = A
            console.run(command: "go")
        }
    "#;
    assert!(compile_str(src, &CompileOptions::default()).is_err());
}
#[test]
fn rejects_duplicate_type_decls_semantically() {
    let src = r#"
        workflow "DuplicateTypes" v1 {
            type Payload = string
            type Payload = integer
            console.run(command: "go")
        }
    "#;
    let diagnostics = analyze_source(src).expect("analyze");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.is_error()
                && diagnostic
                    .message
                    .contains("duplicate type declaration 'Payload'")
        }),
        "diagnostics: {diagnostics:?}"
    );
    let (_, message) = expect_semantic(src);
    assert!(message.contains("duplicate type declaration 'Payload'"));
}
#[test]
fn provider_metadata_infers_action_result_types() {
    let src = r#"
        workflow "ProviderTypes" v1 {
            node tickets <- jira.search(jql: "project = RUNI")
            for ticket in tickets.issues limit none {
                console.run(command: ticket.key)
            }
        }
    "#;
    let issue_type =
        RuninatorType::open_structure([("key", RuninatorType::String)], RuninatorType::Any);
    let providers = vec![
        ProviderMetadata {
            name: "jira".into(),
            actions: vec![
                ActionMetadata::new("search", "Search Jira issues")
                    .with_results(vec![ResultMetadata::new(
                        "issues",
                        RuninatorType::array(issue_type),
                    )])
                    .with_parameters(vec![ParameterMetadata::required(
                        "jql",
                        RuninatorType::String,
                    )]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        },
        ProviderMetadata {
            name: "console".into(),
            actions: vec![
                ActionMetadata::new("run", "Run command").with_parameters(vec![
                    ParameterMetadata::required("command", RuninatorType::Any),
                ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        },
    ];
    let options = CompileOptions {
        providers,
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with provider metadata");
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/rexrap/type_hints/tickets/fields/issues/ty/type")
            .and_then(Value::as_str),
        Some("array")
    );
}
#[test]
fn constrained_types_and_returns_lower_and_round_trip() {
    let src = r#"
        workflow "Typed" v1 returns { url: string, env: enum["dev", "prod"] } {
            params {
                env: enum["dev", "prod"]
                retries: integer range 0..10
                delay: duration range 1s..1h
            }
            console.run(command: "ok")
        }
    "#;
    let definition = compile(src);
    assert_eq!(
        definition
            .input_type
            .to_json_schema()
            .pointer("/properties/env/enum/0"),
        Some(&Value::from("dev"))
    );
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/rexrap/output_type/fields/url/ty/type")
            .and_then(Value::as_str),
        Some("string")
    );
    let rexrap = decompile(&definition).expect("decompile");
    assert!(rexrap.contains("returns {"), "{rexrap}");
    assert!(rexrap.contains("url: string"), "{rexrap}");
    assert!(rexrap.contains("env: enum[\"dev\", \"prod\"]"), "{rexrap}");
    assert!(rexrap.contains("integer range 0..10"), "{rexrap}");
    assert!(rexrap.contains("duration range 1..3600"), "{rexrap}");
}
#[test]
fn strict_provider_arguments_are_checked() {
    let providers = vec![ProviderMetadata {
        name: "demo".into(),
        actions: vec![ActionMetadata::new("run", "Run demo").with_parameters(vec![
            ParameterMetadata::required("count", RuninatorType::Integer),
        ])],
        metadata: ProviderRuntimeMetadata::default(),
    }];
    let options = CompileOptions {
        providers,
        ..CompileOptions::default()
    };

    let missing = compile_str(r#"workflow "Bad" v1 { demo.run() }"#, &options).unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("missing required parameter 'count'"),
        "{missing}"
    );
    let wrong =
        compile_str(r#"workflow "Bad" v1 { demo.run(count: "no") }"#, &options).unwrap_err();
    assert!(
        wrong.to_string().contains("expected integer, got string"),
        "{wrong}"
    );
    let unknown = compile_str(
        r#"workflow "Bad" v1 { demo.run(count: 1, extra: 2) }"#,
        &options,
    )
    .unwrap_err();
    assert!(
        unknown.to_string().contains("unknown parameter 'extra'"),
        "{unknown}"
    );
}
#[test]
fn strict_subflow_requires_signature_and_types_state() {
    let parent = r#"
        workflow "Parent" v1 {
            node child <- subflow("Child", params: { id: "RUNI-1" })
            console.run(command: child.state.url)
        }
    "#;
    let err = compile_str(parent, &CompileOptions::default()).unwrap_err();
    assert!(
        err.to_string().contains("unknown subflow target 'Child'"),
        "{err}"
    );

    let child = r#"
        workflow "Child" v1 returns { url: string } {
            params { id: string }
            console.run(command: params.id)
        }
    "#;
    let options = CompileOptions {
        workflow_signatures: workflow_signature_from_source(child).expect("child signature"),
        ..CompileOptions::default()
    };
    compile_str(parent, &options).expect("typed subflow state compiles");

    let bad_state = r#"
        workflow "Parent" v1 {
            node child <- subflow("Child", params: { id: "RUNI-1" })
            console.run(command: child.state.missing)
        }
    "#;
    let err = compile_str(bad_state, &options).unwrap_err();
    assert!(err.to_string().contains("unknown field 'missing'"), "{err}");

    let bad_params = r#"workflow "Parent" v1 { subflow("Child", params: { id: 7 }) }"#;
    let err = compile_str(bad_params, &options).unwrap_err();
    assert!(
        err.to_string()
            .contains("subflow 'Child' parameters expected string, got integer"),
        "{err}"
    );
}
#[test]
fn detached_subflow_state_is_unavailable() {
    // a detached subflow is fire-and-forget, so its `state` snapshot is never populated. even with
    // the callee signature known, referencing a field off `.state` is an author-time error.
    let child = r#"
        workflow "Child" v1 returns { url: string } {
            params { id: string }
            console.run(command: params.id)
        }
    "#;
    let options = CompileOptions {
        workflow_signatures: workflow_signature_from_source(child).expect("child signature"),
        ..CompileOptions::default()
    };
    let detached = r#"
        workflow "Parent" v1 {
            node child <- subflow("Child", params: { id: "RUNI-1" }, detached: true)
            console.run(command: child.state.url)
        }
    "#;
    let err = compile_str(detached, &options).unwrap_err();
    assert!(
        err.to_string()
            .contains("cannot access field 'url' on null"),
        "{err}"
    );

    // the same reference against an awaited subflow types cleanly from the signature.
    let awaited = r#"
        workflow "Parent" v1 {
            node child <- subflow("Child", params: { id: "RUNI-1" })
            console.run(command: child.state.url)
        }
    "#;
    compile_str(awaited, &options).expect("awaited subflow state resolves");
}
#[test]
fn cast_lets_parse_json_adopt_a_shape() {
    // parse_json is opaque (`any`); an `as` cast asserts a concrete shape, so field access off the
    // result is typed and a wrong field is an author-time error.
    let ok = r#"
        workflow "Cast" v1 {
            params { raw: string }
            do {
                let data = std.encoding.parse_json(params.raw) as { id: integer }
                return { id: data.id }
            }
        }
    "#;
    compile_str(ok, &CompileOptions::default()).expect("cast-typed field resolves");

    let bad = r#"
        workflow "Cast" v1 {
            params { raw: string }
            do {
                let data = std.encoding.parse_json(params.raw) as { id: integer }
                return { id: data.missing }
            }
        }
    "#;
    let err = compile_str(bad, &CompileOptions::default()).unwrap_err();
    assert!(err.to_string().contains("unknown field 'missing'"), "{err}");
}
#[test]
fn cast_rejects_incompatible_concrete_value() {
    // a cast is a type assertion, not a coercion: casting a concrete value to an incompatible type
    // is a genuine mistake and is rejected (an opaque `any` inner would pass, which is the point).
    let src = r#"
        workflow "Cast" v1 {
            do {
                let bad = 5 as string
                return { out: bad }
            }
        }
    "#;
    let err = compile_str(src, &CompileOptions::default()).unwrap_err();
    assert!(
        err.to_string()
            .contains("cast expected string, got integer"),
        "{err}"
    );
}
#[test]
fn cast_round_trips() {
    // the cast is erased at lowering, so the compiled graph round-trips (the `as T` is author-time).
    let src = r#"
        workflow "Cast" v1 {
            params { raw: string }
            do {
                let data = std.encoding.parse_json(params.raw) as { id: integer }
                return { id: data.id }
            }
        }
    "#;
    assert_round_trips(src);
}
#[test]
fn function_type_annotation_checks_and_round_trips() {
    // a `function<(A) -> R>` annotation binds the lambda's parameter to `A`, so the body checks
    // against the annotation and a later application resolves.
    let ok = r#"
        workflow "Fn" v1 {
            do {
                let inc: function<(integer) -> integer> = x => x + 1
                return { out: inc(2) }
            }
        }
    "#;
    compile_str(ok, &CompileOptions::default()).expect("typed lambda binds and applies");
    assert_round_trips(ok);

    // a lambda whose body conflicts with the declared return type is rejected.
    let bad = r#"
        workflow "Fn" v1 {
            do {
                let inc: function<(integer) -> integer> = x => "not a number"
                return { out: inc(2) }
            }
        }
    "#;
    let err = compile_str(bad, &CompileOptions::default()).unwrap_err();
    assert!(err.to_string().contains("compute local 'inc'"), "{err}");
}
#[test]
fn applies_a_field_held_closure_and_round_trips() {
    // a closure stored in an object field is applied with `(obj.f)(args)`; the parenthesized callee
    // keeps it from re-parsing as a `obj.f(args)` method call, and it round-trips.
    let src = r#"
        workflow "Apply" v1 {
            do {
                let ops = { inc: x => x + 1 }
                return { out: (ops.inc)(5) }
            }
        }
    "#;
    compile_str(src, &CompileOptions::default()).expect("field-held closure applies");
    assert_round_trips(src);
}
#[test]
fn applies_an_index_held_closure_and_round_trips() {
    // a closure stored in an array element is applied with `fns[0](args)`.
    let src = r#"
        workflow "Apply" v1 {
            do {
                let fns = [x => x + 1, x => x + 2]
                return { out: fns[0](10) }
            }
        }
    "#;
    compile_str(src, &CompileOptions::default()).expect("index-held closure applies");
    assert_round_trips(src);
}
#[test]
fn applied_closure_checks_arity() {
    // applying a known-arity closure with the wrong argument count is an author-time error.
    let src = r#"
        workflow "Apply" v1 {
            do {
                let ops = { inc: x => x + 1 }
                return { out: (ops.inc)(1, 2) }
            }
        }
    "#;
    let err = compile_str(src, &CompileOptions::default()).unwrap_err();
    assert!(
        err.to_string()
            .contains("applied function expects 1 argument(s), got 2"),
        "{err}"
    );
}
#[test]
fn round_trips_let_type_annotation() {
    let src = r#"
        workflow "Typed" v1 {
            node probe: { count: integer } <- console.run(command: "probe")
            console.run(command: "after ${probe.count}")
        }
    "#;
    assert_round_trips(src);
    // the declared type survives compile -> decompile and re-appears in the source.
    let rexrap = decompile(&compile(src)).expect("decompile");
    assert!(
        rexrap.contains("node probe:"),
        "annotation missing:\n{rexrap}"
    );
}

// expression-granular spans -------------------------------------------------
