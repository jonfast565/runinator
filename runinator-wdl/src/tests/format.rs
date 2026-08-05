//! the formatter: normalizing authored source and staying idempotent on its own output.

use super::*;

#[test]
fn format_normalizes_wdl_source() {
    let src = r#"workflow "Fmt"   v1{params{jira:{base_url:string,email?:string}, "odd-key": map<string[]>, fallback?: string, enabled: boolean, retry: integer, transitions:{done:string,in_progress:string,in_review:string}}
@skip node first: { output: string, status: string, items: string[] } <- console.run(command:"echo ${params.jira.base_url}"++(params.fallback??"none"), transitions:{done:"done",in_progress:"progress",in_review:"review"}).timeout(30s).retry(2).tags("ci","fmt").mcp()
fail -> cleanup
timeout -> fail
if params.enabled==true&&exists first.output{emit "ready"{value:first.output}}else{wait 5s}
match first.status{"ok"->console.run(command:"ok") when params.retry > 0 -> {console.run(command:"retry")} else -> fail "bad"}
parallel{branch{console.run(command:"a")}branch{console.run(command:"b")}}join any
try{console.run(command:"risky")}catch{console.run(command:"recover")}finally{console.run(command:"done")}
race winner first_success{branch{console.run(command:"primary")}branch{console.run(command:"backup")}}
map item in first.items concurrency 2{console.run(command:string(item))}
node cleanup <- console.run(command:"cleanup")
jira.transition(base_url:params.jira.base_url,email:params.jira.email,key:first.output,token:"secret",transition_id:params.transitions.in_progress).timeout(30s)
}"#;

    let formatted = format_str(src).expect("format");
    let expected = r#"workflow "Fmt" v1.0.0 {
    params {
        jira: {
            base_url: string,
            email?: string
        }
        "odd-key": map<string[]>
        fallback?: string
        enabled: boolean
        retry: integer
        transitions: {
            done: string,
            in_progress: string,
            in_review: string
        }
    }

    @skip
    node first: { output: string, status: string, items: string[] } <- console.run(
        command: "echo ${params.jira.base_url}" ++ (params.fallback ?? "none"),
        transitions: {
            done: "done",
            in_progress: "progress",
            in_review: "review"
        }
    ).timeout(30s)
     .retry(2)
     .tags("ci", "fmt")
     .mcp()
    edges {
        fail -> cleanup
        timeout -> fail
    }

    if params.enabled == true && exists first.output {
        emit "ready" {
            value: first.output
        }
    } else {
        wait 5s
    }

    match first.status {
        "ok" -> {
            console.run(
                command: "ok"
            )
        }
        when params.retry > 0 -> {
            console.run(
                command: "retry"
            )
        }
        else -> {
            fail "bad"
        }
    }

    parallel {
        branch {
            console.run(
                command: "a"
            )
        }
        branch {
            console.run(
                command: "b"
            )
        }
    } join any

    try {
        console.run(
            command: "risky"
        )
    } catch {
        console.run(
            command: "recover"
        )
    } finally {
        console.run(
            command: "done"
        )
    }

    race winner first_success {
        branch {
            console.run(
                command: "primary"
            )
        }
        branch {
            console.run(
                command: "backup"
            )
        }
    }

    map item in first.items concurrency 2 {
        console.run(
            command: string(item)
        )
    }

    node cleanup <- console.run(
        command: "cleanup"
    )

    jira.transition(
        base_url: params.jira.base_url,
        email: params.jira.email,
        key: first.output,
        token: "secret",
        transition_id: params.transitions.in_progress
    ).timeout(30s)
}
"#;

    assert_eq!(formatted, expected);
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
    let first = compile(src);
    let second = compile_str(&formatted, &CompileOptions::default()).expect("compile formatted");
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition),
        runinator_workflows::normalize_definition(second.definition)
    );
}
#[test]
fn formats_toggle_and_split_idempotently() {
    let src = r#"
        workflow "Rollout" v1 {
            node seed <- console.run(command: "seed")
            toggle config.flags.new_checkout {
                on -> { console.run(command: "new") }
                off -> { console.run(command: "old") }
            }
            split on seed.user_id {
                30% -> { console.run(command: "variant_a") }
                70% -> { console.run(command: "variant_b") }
                else -> { console.run(command: "control") }
            }
            node done <- console.run(command: "done")
        }
    "#;
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("toggle "), "toggle kept:\n{formatted}");
    assert!(formatted.contains("split on "), "split kept:\n{formatted}");
    assert!(formatted.contains("30% -> {"), "weight kept:\n{formatted}");
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
    compile_str(&formatted, &CompileOptions::default()).expect("compile formatted");
}
#[test]
fn format_parenthesizes_eventless_scalar_output() {
    // an event-less scalar payload must keep its parens through formatting, otherwise it would
    // be re-parsed as the event type and silently lose the payload.
    let src = r#"workflow "E" { emit ("ready") }"#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("emit (\"ready\")"),
        "parens preserved:\n{formatted}"
    );
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);

    let first = compile(src);
    let second = compile_str(&formatted, &CompileOptions::default()).expect("compile formatted");
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition),
        runinator_workflows::normalize_definition(second.definition)
    );
}
