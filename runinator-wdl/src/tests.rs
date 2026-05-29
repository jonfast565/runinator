use crate::parse_document;

#[test]
fn parses_minimal_workflow() {
    let src = r#"
        workflow "Hello" v1 {
            console.run(command: "echo hi")
        }
    "#;
    let doc = parse_document(src).expect("parse");
    assert_eq!(doc.workflow.name, "Hello");
    assert_eq!(doc.workflow.version, Some(1));
    assert_eq!(doc.workflow.body.len(), 1);
}

#[test]
fn parses_kitchen_sink() {
    let src = r#"
        workflow "Kitchen Sink" v2 {
            input {
                jira: { base_url: string, email: string, token: string, jql: string }
                github?: { token: string }
                shards: string[]
                labels: map<string>
                payload: { kind: string } | null
            }

            let tickets = jira.search(
                base_url: input.jira.base_url,
                jql:      input.jira.jql,
            ).timeout(60s).retry(3).tags("ci", "release").mcp()

            if tickets.count > 0 && input.jira.jql contains "P0" {
                emit "found" { count: tickets.count }
            } else if exists github.token {
                console.run(command: "noop")
            } else {
                wait 30s until "ready"
            }

            for ticket in tickets.issues limit 50 {
                spawn "Ticket Work" detached reuse
                    as "Ticket Work: ${ticket.key}"
                    with { ticket, parent: run.run_id }
            }
            -> done

            match input.payload.kind {
                "fanout" -> { console.run(command: "a") }
                when input.shards contains "x" -> console.run(command: "b")
                else -> { emit "default" { } }
            }

            parallel {
                branch { console.run(command: "lint") }
                branch { console.run(command: "test") }
            } join all -> report

            race winner first_success {
                branch { console.run(command: "primary") }
                branch { console.run(command: "backup") }
            }

            map shard in input.shards concurrency 4 {
                console.run(command: "reindex ${shard}")
            }

            try {
                console.run(command: "risky")
            } catch {
                console.run(command: "rollback")
            } finally {
                console.run(command: "cleanup")
            }

            approve "Ship it?" type "change_request" { env: "prod" }
                ok -> deploy
                reject -> abort

            let deploy = console.run(command: "deploy")
                ok -> done
                fail -> abort

            let abort = console.run(command: "abort")
            let report = console.run(command: "report")

            set name = "renamed: ${tickets.count}"
            fail "done with errors"
        }
    "#;
    let doc = parse_document(src).expect("parse kitchen sink");
    assert_eq!(doc.workflow.name, "Kitchen Sink");
    assert!(doc.workflow.input.is_some());
    assert!(doc.workflow.body.len() >= 12);
}
