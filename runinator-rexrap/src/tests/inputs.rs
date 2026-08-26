//! workflow input fields: defaults (literal and expression), open structs, and the defaults
//! applied to a run's parameters.

use super::*;

#[test]
fn input_default_literal_lowers_and_round_trips() {
    let src = r#"
        workflow "Defaults" v1 {
            params {
                count: integer = 5
                label: string = "hello"
            }

            do {
                console.run(command: "go ${params.label}")
            }
        }
    "#;
    let def = compile(src);
    let RuninatorType::Struct { fields, .. } = &def.input_type else {
        panic!("expected struct input, got {:?}", def.input_type);
    };
    let count = fields.get("count").expect("count field");
    assert_eq!(count.default, Some(Value::from(5)));
    // a defaulted field is treated as optional.
    assert!(!count.required, "defaulted field should be optional");

    let rexrap = decompile(&def).expect("decompile");
    assert!(rexrap.contains("count: integer = 5"), "{rexrap}");
    assert!(rexrap.contains(r#"label: string = "hello""#), "{rexrap}");
    let second = compile_str(&rexrap, &CompileOptions::default()).expect("recompile");
    assert_eq!(def.input_type, second.input_type);
}
#[test]
fn input_default_expression_round_trips() {
    let src = r#"
        workflow "Defaults" v1 {
            params {
                base: string = config.api.base_url
                token: string = secret.api.token
                full: string = config.api.base_url ++ "/v1"
            }

            do {
                console.run(command: params.base)
            }
        }
    "#;
    let def = compile(src);
    let rexrap = decompile(&def).expect("decompile");
    assert!(rexrap.contains("base: string = api.base_url"), "{rexrap}");
    assert!(
        rexrap.contains("token: string = api.secret.token"),
        "{rexrap}"
    );
    assert!(
        rexrap.contains(r#"full: string = api.base_url ++ "/v1""#),
        "{rexrap}"
    );
    let second = compile_str(&rexrap, &CompileOptions::default()).expect("recompile");
    assert_eq!(def.input_type, second.input_type);
}
#[test]
fn open_input_struct_lowers_and_round_trips() {
    let src = r#"
        workflow "Open" v1 {
            params {
                name: string
                ...: integer
            }

            do {
                console.run(command: params.name)
            }
        }
    "#;
    let def = compile(src);
    let RuninatorType::Struct { additional, .. } = &def.input_type else {
        panic!("expected struct input, got {:?}", def.input_type);
    };
    assert_eq!(
        additional.as_deref(),
        Some(&runinator_models::types::RuninatorType::Integer)
    );

    let rexrap = decompile(&def).expect("decompile");
    assert!(rexrap.contains("...: integer"), "{rexrap}");
    let second = compile_str(&rexrap, &CompileOptions::default()).expect("recompile");
    assert_eq!(def.input_type, second.input_type);
}
#[test]
fn rejects_input_default_referencing_prev() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { x: string = prev.foo }

            do {
                console.run(command: params.x)
            }
        }
    "#,
    );
    assert!(
        message.contains("parameter default may only reference"),
        "{message}"
    );
}
#[test]
fn apply_input_defaults_fills_missing_fields() {
    let src = r#"
        workflow "Defaults" v1 {
            params {
                count: integer = 5
                label: string = "n-" ++ string(params.count)
                provided: string
            }

            do {
                console.run(command: params.label)
            }
        }
    "#;
    let def = compile(src);
    let mut context = Value::from(serde_json::json!({
        "input": { "provided": "yes" },
        "steps": {},
    }));
    runinator_workflows::apply_input_defaults(&mut context, &def.input_type);
    let input = context.get("input").expect("input slot");
    assert_eq!(input.get("count"), Some(&Value::from(5)));
    assert_eq!(input.get("label"), Some(&Value::from("n-5")));
    // a provided value is never overwritten.
    assert_eq!(input.get("provided"), Some(&Value::from("yes")));
}
#[test]
fn apply_input_defaults_synthesizes_input_when_absent() {
    let src = r#"
        workflow "Defaults" v1 {
            params { greeting: string = "hi" }

            do {
                console.run(command: params.greeting)
            }
        }
    "#;
    let def = compile(src);
    let mut context = Value::from(serde_json::json!({ "steps": {} }));
    runinator_workflows::apply_input_defaults(&mut context, &def.input_type);
    assert_eq!(
        context.get("input").and_then(|i| i.get("greeting")),
        Some(&Value::from("hi"))
    );
}
#[test]
fn format_renders_input_defaults() {
    let src = "workflow \"D\" v1 {\nparams {\ncount: integer = 5\nbase: string = app.x\n}\nimport settings app as app\n\n    do {\n    console.run(command: params.base)\n    }\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("count: integer = 5"), "{formatted}");
    assert!(formatted.contains("base: string = app.x"), "{formatted}");
    // formatted source still compiles to the same parameter type.
    let a = compile(src);
    let b = compile_str(&formatted, &CompileOptions::default()).expect("compile formatted");
    assert_eq!(a.input_type, b.input_type);
}

// .rexraps secrets format ------------------------------------------------------
