//! the parser against a minimal document and a kitchen-sink one.

use super::*;

#[test]
fn parses_minimal_workflow() {
    let src = r#"
        workflow "Hello" v1 {
            console.run(command: "echo hi")
        }
    "#;
    let doc = parse_document(src).expect("parse");
    let workflow = doc.single_workflow().expect("one workflow");
    assert_eq!(workflow.name, "Hello");
    assert_eq!(
        workflow.version,
        Some(runinator_models::semver::SemVer::new(1, 0, 0))
    );
    assert_eq!(workflow.body.len(), 1);
}
#[test]
fn parses_kitchen_sink() {
    let src = r#"
        workflow "Kitchen Sink" v2 {
            params {
                jira: { base_url: string, email: string, token: string, jql: string }
                github?: { token: string }
                shards: string[]
                labels: map<string>
                payload: { kind: string } | null
            }

            node tickets <- jira.search(
                base_url: params.jira.base_url,
                jql:      params.jira.jql,
            ).timeout(60s).retry(3).tags("ci", "release").mcp()

            if tickets.count > 0 && params.jira.jql contains "P0" {
                emit "found" { count: tickets.count }
            } else if exists github.token {
                console.run(command: "noop")
            } else {
                wait 30s until "ready"
            }

            for ticket in tickets.issues limit 50 {
                subflow("Ticket Work", params: { ticket, parent: run.run_id }, detached: true, reuse: true, name: "Ticket Work: ${ticket.key}")
            }
            -> done

            match params.payload.kind {
                "fanout" -> { console.run(command: "a") }
                when params.shards contains "x" -> console.run(command: "b")
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

            map shard in params.shards concurrency 4 {
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

            node deploy <- console.run(command: "deploy")
                ok -> done
                fail -> abort

            node abort <- console.run(command: "abort")
            node report <- console.run(command: "report")

            set name = "renamed: ${tickets.count}"
            fail "done with errors"
        }
    "#;
    let doc = parse_document(src).expect("parse kitchen sink");
    let workflow = doc.single_workflow().expect("one workflow");
    assert_eq!(workflow.name, "Kitchen Sink");
    assert!(workflow.input.is_some());
    assert!(workflow.body.len() >= 12);
}
