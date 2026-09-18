//! covers parsing for scriptable command-center parity commands.

use super::*;

const ID: &str = "00000000-0000-0000-0000-000000000001";

fn parses(args: &[&str]) {
    let argv = std::iter::once("runinatorctl").chain(args.iter().copied());
    Cli::try_parse_from(argv).unwrap_or_else(|error| panic!("{args:?}: {error}"));
}

#[test]
fn parses_workflow_and_run_recovery_commands() {
    for args in [
        vec![
            "workflows",
            "simulate",
            "workflow.json",
            "--input-file",
            "input.json",
        ],
        vec!["workflows", "contract-impact", "workflow.json"],
        vec!["workflows", "import-archive", "pack.zip", "--overwrite"],
        vec!["workflows", "enable", "daily"],
        vec!["workflows", "disable", "daily"],
        vec!["workflows", "delete", "daily"],
        vec!["runs", "step", ID, "--cursor", ID],
        vec!["runs", "continue", ID],
        vec!["runs", "breakpoints", "set", ID, "--breakpoint", "review"],
        vec!["runs", "to-node", ID, ID, "publish"],
        vec!["runs", "pause-on-failure", ID, "true"],
        vec![
            "runs",
            "signal",
            ID,
            "approved",
            "--json-file",
            "signal.json",
        ],
        vec!["runs", "interrupt", ID, "operator"],
        vec!["runs", "resolve-input", ID, "--json-file", "result.json"],
        vec!["runs", "terminal", "resize", ID, "120", "40"],
        vec![
            "artifacts",
            "download",
            "--effect",
            ID,
            "--event",
            ID,
            "--output",
            "artifact.bin",
        ],
    ] {
        parses(&args);
    }
}

#[test]
fn parses_authoring_and_lifecycle_commands() {
    for args in [
        vec!["triggers", "show", ID],
        vec!["triggers", "apply", "trigger.json"],
        vec!["triggers", "delete", ID],
        vec!["freeze", "update", ID, "window.json"],
        vec![
            "freeze",
            "calendar",
            "subscribe",
            "--scope",
            "organization",
            "--org-id",
            ID,
        ],
        vec!["freeze", "calendar", "download", "--output", "calendar.ics"],
        vec![
            "files",
            "upload",
            "source.csv",
            "--path",
            "fixtures/source.csv",
        ],
        vec!["files", "download", ID, "--output", "source.csv"],
        vec!["notebooks", "sessions", "rename", ID, "investigation"],
        vec!["notebooks", "cells", "create", ID, "cell.rrx"],
        vec!["notebooks", "cells", "run", ID],
        vec![
            "settings", "move", ID, "--scope", "team:ops", "--name", "token", "--kind", "secret",
        ],
        vec!["workspaces", "delete", ID, "--version", "2"],
    ] {
        parses(&args);
    }
}

#[test]
fn parses_operational_control_commands() {
    for args in [
        vec!["notifications", "list", "--unread"],
        vec![
            "notifications",
            "policies",
            "apply",
            "policy.json",
            "--id",
            ID,
        ],
        vec!["gates", "list", "--run-id", ID, "--status", "waiting"],
        vec!["gates", "open", ID, "--reason", "approved"],
        vec!["orchestrations", "epochs", ID],
        vec!["orchestrations", "operations", "list", ID],
        vec![
            "orchestrations",
            "operations",
            "resolve",
            ID,
            ID,
            "succeeded",
            "--reason",
            "verified",
        ],
        vec!["orchestrations", "debug", "step", ID],
        vec!["orchestrations", "adapters", "inspection-set", ID, "review"],
        vec![
            "orchestrations",
            "adapters",
            "delivery-decide",
            ID,
            ID,
            "approve",
        ],
        vec!["ingress", "external", "configure", "workflow", ID, "paused"],
        vec![
            "ingress",
            "broker",
            "session",
            "organization",
            "--scope-id",
            ID,
        ],
        vec!["ingress", "messages", "list", "--adapter", ID],
        vec!["ingress", "dead-letters", "list", "--channel", "ingress"],
        vec!["audit", "list", "--actor", ID],
        vec!["records", "external-items"],
        vec!["records", "events"],
    ] {
        parses(&args);
    }
}
