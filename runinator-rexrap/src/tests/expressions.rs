//! operators and their surface: comparisons and ternaries lowering to intrinsic calls, and the
//! form they format back to.

use super::*;
use runinator_rexrap_syntax::ast::{ExprKind, StrPart, StringStyle};

#[test]
fn string_literals_preserve_their_authored_style_and_content() {
    let cases = [
        ("\"hello ${params.name}\"", StringStyle::Quoted, "hello ", 1),
        (
            "\"\"\"first\n    second ${params.name}\"\"\"",
            StringStyle::Multiline,
            "first\n    second ",
            1,
        ),
        (
            r#"@"C:\work\${literal} ""quoted""""#,
            StringStyle::Verbatim,
            r#"C:\work\${literal} "quoted""#,
            0,
        ),
        (
            "@\"\"\"first\n    ${literal} \\\\ path\n\"\"\"",
            StringStyle::VerbatimMultiline,
            "first\n    ${literal} \\\\ path\n",
            0,
        ),
    ];

    for (source, style, expected_text, expected_interpolations) in cases {
        let expr = crate::parse_expression_fragment(source).expect("parse string");
        let ExprKind::Str(literal) = expr.kind else {
            panic!("expected string literal");
        };
        assert_eq!(literal.style, style, "{source}");
        let text = literal
            .parts
            .iter()
            .filter_map(|part| match part {
                StrPart::Lit(text) => Some(text.as_str()),
                StrPart::Expr(_) => None,
            })
            .collect::<String>();
        assert_eq!(text, expected_text, "{source}");
        assert_eq!(
            literal
                .parts
                .iter()
                .filter(|part| matches!(part, StrPart::Expr(_)))
                .count(),
            expected_interpolations,
            "{source}"
        );
    }
}

#[test]
fn string_literal_forms_lower_to_the_same_value() {
    let options = CompileOptions::default();
    let quoted = crate::lower_fragment(r#""path\\ops""#, RexRapFragmentKind::Expression, &options)
        .expect("lower quoted");
    let verbatim =
        crate::lower_fragment(r#"@"path\ops""#, RexRapFragmentKind::Expression, &options)
            .expect("lower verbatim");
    assert_eq!(quoted, verbatim);

    let multiline = crate::lower_fragment(
        "\"\"\"first\n    second\"\"\"",
        RexRapFragmentKind::Expression,
        &options,
    )
    .expect("lower multiline");
    let verbatim_multiline = crate::lower_fragment(
        "@\"\"\"first\n    second\"\"\"",
        RexRapFragmentKind::Expression,
        &options,
    )
    .expect("lower verbatim multiline");
    assert_eq!(multiline, verbatim_multiline);
}

#[test]
fn string_literal_delimiters_handle_embedded_quotes_and_plain_string_positions() {
    let expr = crate::parse_expression_fragment("\"\"\"before \\\"\"\" after\"\"\"")
        .expect("escaped triple delimiter");
    let ExprKind::Str(literal) = expr.kind else {
        panic!("expected string literal");
    };
    assert_eq!(
        literal.parts,
        vec![StrPart::Lit("before \"\"\" after".into())]
    );

    let document =
        parse_document(r#"workflow @"String ""Form""" { do {} }"#).expect("verbatim workflow name");
    assert_eq!(document.workflows[0].name, "String \"Form\"");
    assert!(crate::parse_expression_fragment("@\"\"\"before \"\"\" after\"\"\"").is_err());
    assert!(crate::parse_expression_fragment("\"\"\"unterminated").is_err());
}

#[test]
fn multiline_and_verbatim_strings_format_and_decompile_canonically() {
    let source = r#"
        workflow @"String Forms" v1 {
            do {
                let write = console.run(
                    body: """first
    second ${params.name}""",
                    path: @"C:\work\reports"
                )
            }
        }
    "#;
    let formatted = format_str(source).expect("format");
    assert!(
        formatted.contains("workflow \"String Forms\" v1"),
        "{formatted}"
    );
    assert!(
        formatted.contains("\"\"\"first\n    second ${params.name}\"\"\""),
        "{formatted}"
    );
    assert!(formatted.contains("@\"C:\\work\\reports\""), "{formatted}");
    assert_eq!(format_str(&formatted).expect("reformat"), formatted);

    let definition = compile(source);
    let decompiled = decompile(&definition).expect("decompile");
    assert!(
        decompiled.contains("\"\"\"first\n    second"),
        "{decompiled}"
    );
    assert!(
        decompiled.contains("@\"C:\\work\\reports\""),
        "{decompiled}"
    );
    assert_round_trips(source);
}

#[test]
fn comparison_operators_lower_to_intrinsic_calls() {
    let src = r#"
        workflow "Cmp" v1 {

            do {
                let go = console.run(le: params.x <= 1, eq: params.y == params.z, gt: params.a > 2)
            }
        }
    "#;
    let definition = compile(src);
    for (key, intrinsic) in [("le", "lte"), ("eq", "eq"), ("gt", "gt")] {
        let value = action_config_value(&definition, key);
        let call = value
            .get("$call")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{key} is not a $call: {value:?}"));
        assert_eq!(call, intrinsic, "{key} lowered to wrong intrinsic");
        let args = value.get("args").and_then(Value::as_array).expect("args");
        assert_eq!(args.len(), 2, "{key} should have two operands");
    }
}
#[test]
fn ternary_lowers_to_if_form() {
    let src = r#"
        workflow "Tern" v1 {

            do {
                let go = console.run(size: params.n <= 1 ? "small" : "big")
            }
        }
    "#;
    let definition = compile(src);
    let value = action_config_value(&definition, "size");
    assert_eq!(
        value.get("then").and_then(Value::as_str),
        Some("small"),
        "{value:?}"
    );
    assert_eq!(value.get("else").and_then(Value::as_str), Some("big"));
    let cond = value.get("$if").expect("$if branch");
    assert_eq!(cond.get("$call").and_then(Value::as_str), Some("lte"));
}
#[test]
fn ternary_round_trips_through_formatter() {
    let src = "workflow \"Tern\" v1 {\n\n    do {\n        let go = console.run(size: params.n <= 1 ? \"small\" : \"big\")\n    }\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("params.n <= 1 ? \"small\" : \"big\""),
        "{formatted}"
    );
}
#[test]
fn comparison_round_trips_through_formatter() {
    let src = "workflow \"Cmp\" v1 {\n\n    do {\n        let go = console.run(flag: params.x >= 2)\n    }\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("params.x >= 2"), "{formatted}");
}
#[test]
fn secret_reference_requires_scope_and_name() {
    let src = r#"
        namespace runinator.tests
        workflow "BadSecret" v1 {
            key bad_secret

            do {
                let go = console.run(command: "x", token: secret.github)
            }
        }
    "#;
    let error = crate::compile_str(src, &CompileOptions::default())
        .expect_err("unimported settings namespace must be rejected");
    assert!(
        error.to_string().contains("typed `import settings"),
        "{error}"
    );
}
#[test]
fn round_trips_fanin_error_handlers_and_convergence() {
    // mirrors the Ticket Work shape: linear steps with `fail ->` handlers, a poll loop, an
    // if/approval branch, and several handlers converging on a shared cleanup node. exercises
    // the decompiler's worklist + back-arrow handling for arbitrary fan-in.
    let src = r#"
        workflow "Fanin" v1 {
            params { poll: { interval: integer } }

            do {
                let prepare = console.run(command: "prepare")
                    routes {
                        on failure {
                            continue notify_failure
                        }
                    }
                let build = console.run(command: "build")
                    routes {
                        on failure {
                            continue notify_failure
                        }
                    }
                until check.status == "passed" || check.status == "failed" limit 20 {
                    wait params.poll.interval
                    let check = console.run(command: "poll")
                }

                if check.status == "passed" {
                    approve "ship it?" type "merge"
                        routes {
                            on success {
                                continue finalize
                            }
                            on reject {
                                continue rollback
                            }
                        }
                }
                routes {
                    on next {
                        continue notify_failure
                    }
                }

                let finalize = console.run(command: "finalize")
                    routes {
                        on failure {
                            continue notify_failure
                        }
                    }
                let report = console.run(command: "report")
                    routes {
                        on next {
                            continue cleanup
                        }
                    }
                let rollback = console.run(command: "rollback")
                    routes {
                        on next {
                            continue cleanup
                        }
                    }
                let notify_failure = console.run(command: "alert")
                    routes {
                        on next {
                            continue cleanup
                        }
                    }
                let cleanup = console.run(command: "cleanup")
                    routes {
                        on next {
                            continue end
                        }
                    }
            }
        }
    "#;
    assert_round_trips_unordered(src);
}
