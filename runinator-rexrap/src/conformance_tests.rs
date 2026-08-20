//! exhaustive node-kind conformance for the rexrap surface.
//!
//! `AGENTS.md` requires the grammar to round-trip every node kind's parameters, but nothing in the
//! compiler links `WorkflowNodeKind` to the grammar: a kind added to the model can lower with no
//! surface syntax, or round-trip while silently dropping a parameter, and every exhaustive `match`
//! still compiles. these tests are that link. they iterate `WorkflowNodeKind::ALL`, so a new kind
//! fails here until it is either given a fixture or listed in `NO_REXRAP_SURFACE`.

use runinator_models::providers::RuninatorType;
use runinator_models::workflows::{WorkflowDefinition, WorkflowNodeKind};

use crate::{CompileOptions, RexRapError, WorkflowSignature, compile_str, decompile};

/// compile options carrying the signatures the subflow fixture calls into.
///
/// The surface has one compute form: `do {}` always compiles to an invocation node.
fn options(_kind: &WorkflowNodeKind) -> CompileOptions {
    CompileOptions {
        workflow_signatures: vec![WorkflowSignature {
            name: "Child".to_string(),
            input: RuninatorType::Any,
            output: RuninatorType::Any,
        }],
        ..CompileOptions::default()
    }
}

fn compile(kind: &WorkflowNodeKind, src: &str) -> Result<WorkflowDefinition, RexRapError> {
    compile_str(src, &options(kind))
}

/// kinds with no author-facing rexrap syntax, each with the reason it has none.
///
/// this list is the point of the suite: adding to it is a decision that shows up in review, rather
/// than an omission that shows up as a user filing a bug about a workflow they cannot write.
const NO_REXRAP_SURFACE: &[(WorkflowNodeKind, &str)] = &[];

/// one rexrap workflow per node kind, written the way an author would write it.
///
/// each fixture must compile to a graph containing at least one node of its kind and survive a
/// compile -> decompile -> compile round trip with an identical normalized graph.
fn fixtures() -> Vec<(WorkflowNodeKind, &'static str)> {
    vec![
        // start and end are implicit in every workflow; the minimal body proves they are emitted.
        (
            WorkflowNodeKind::Start,
            r#"workflow "Conf Start" v1 { node a <- console.run(command: "echo hi") }"#,
        ),
        (
            WorkflowNodeKind::End,
            r#"workflow "Conf End" v1 { node a <- console.run(command: "echo hi") }"#,
        ),
        (
            WorkflowNodeKind::Action,
            r#"workflow "Conf Action" v1 { node a <- console.run(command: "echo hi") }"#,
        ),
        (
            WorkflowNodeKind::Wait,
            r#"workflow "Conf Wait" v1 { wait 30s }"#,
        ),
        // The same `do { }` an author already writes, compiled to bytecode.
        (
            WorkflowNodeKind::Invocation,
            r#"workflow "Conf Invocation" v1 { do { return { total: prev.a } } }"#,
        ),
        // `resume` only ever appears inside an interrupt handler region, so its fixture has to
        // carry the region that gives it meaning.
        (
            WorkflowNodeKind::Resume,
            r#"
            workflow "Conf Resume" v1 {
                interrupt on wake {
                    console.run(command: "echo refresh")
                    resume next
                }

                wait 30s
            }"#,
        ),
        // the other end of the same bracket: `interrupt` has no statement syntax of its own, it is
        // what an `interrupt on` header block lowers its entry to. the fixture is the header, and
        // the round trip is what proves the entry node survives being rendered back as that header.
        (
            WorkflowNodeKind::Interrupt,
            r#"
            workflow "Conf Interrupt" v1 {
                interrupt on wake {
                    console.run(command: "echo refresh")
                    resume next
                }

                wait 30s
            }"#,
        ),
        (
            WorkflowNodeKind::Condition,
            r#"
            workflow "Conf Condition" v1 {
                params { flag: bool }
                if params.flag {
                    console.run(command: "echo yes")
                } else {
                    console.run(command: "echo no")
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Switch,
            r#"
            workflow "Conf Switch" v1 {
                params { env: string }
                match params.env {
                    "prod" -> { console.run(command: "echo prod") }
                    else -> { console.run(command: "echo other") }
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Toggle,
            r#"
            workflow "Conf Toggle" v1 {
                params { flag: bool }
                toggle params.flag {
                    on -> { console.run(command: "echo on") }
                    off -> { console.run(command: "echo off") }
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Percentage,
            r#"
            workflow "Conf Percentage" v1 {
                params { user: string }
                split on params.user {
                    50% -> { console.run(command: "echo a") }
                    else -> { console.run(command: "echo b") }
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Approval,
            r#"workflow "Conf Approval" v1 { approve "ship?" { env: "prod" } }"#,
        ),
        (
            WorkflowNodeKind::Gate,
            r#"workflow "Conf Gate" v1 { gate manual { label: "release" } }"#,
        ),
        (
            WorkflowNodeKind::Signal,
            r#"workflow "Conf Signal" v1 { signal "deploy-approved" { source: "ops" } }"#,
        ),
        (
            WorkflowNodeKind::Loop,
            r#"
            workflow "Conf Loop" v1 {
                params { items: string[] }
                for item in params.items limit none {
                    console.run(command: "echo ${item}")
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Parallel,
            r#"
            workflow "Conf Parallel" v1 {
                parallel {
                    branch { console.run(command: "echo one") }
                    branch { console.run(command: "echo two") }
                } join all
            }"#,
        ),
        (
            WorkflowNodeKind::Join,
            r#"
            workflow "Conf Join" v1 {
                parallel {
                    branch { console.run(command: "echo one") }
                    branch { console.run(command: "echo two") }
                } join all
            }"#,
        ),
        (
            WorkflowNodeKind::Map,
            r#"
            workflow "Conf Map" v1 {
                params { items: string[] }
                map shard in params.items concurrency none {
                    console.run(command: "echo ${shard}")
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Race,
            r#"
            workflow "Conf Race" v1 {
                race winner first_success {
                    branch { console.run(command: "echo primary") }
                    branch { console.run(command: "echo backup") }
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Try,
            r#"
            workflow "Conf Try" v1 {
                try {
                    console.run(command: "echo risky")
                } catch {
                    console.run(command: "echo recover")
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Subflow,
            r#"
            workflow "Conf Subflow" v1 {
                params { id: string }
                subflow("Child", params: { id: params.id })
            }"#,
        ),
        (
            WorkflowNodeKind::Assert,
            r#"
            workflow "Conf Assert" v1 {
                params { amount: int }
                assert {
                    "amount_positive": params.amount > 0
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Transform,
            r#"
            workflow "Conf Transform" v1 {
                params { amount: int }
                transform {
                    doubled = params.amount * 2
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Audit,
            r#"
            workflow "Conf Audit" v1 {
                params { user: string }
                audit action "reviewed" actor params.user
            }"#,
        ),
        (
            WorkflowNodeKind::Checkpoint,
            r#"workflow "Conf Checkpoint" v1 { checkpoint "after-ingest" }"#,
        ),
        (
            WorkflowNodeKind::Mutex,
            r#"workflow "Conf Mutex" v1 { mutex "deploy-lock" every 5s timeout 300s }"#,
        ),
        (
            WorkflowNodeKind::Throttle,
            r#"workflow "Conf Throttle" v1 { throttle "github-api" rate 10 per 60s }"#,
        ),
        (
            WorkflowNodeKind::Cooldown,
            r#"workflow "Conf Cooldown" v1 { cooldown "scan-gate" every 300s }"#,
        ),
        (
            WorkflowNodeKind::AwaitRun,
            r#"
            workflow "Conf Await" v1 {
                params { user: string }
                await workflow "Prep" key params.user mode "all" timeout 1800s
            }"#,
        ),
        (
            WorkflowNodeKind::Debounce,
            r#"workflow "Conf Debounce" v1 { debounce "file-change" delay 30s }"#,
        ),
        (
            WorkflowNodeKind::Collect,
            r#"workflow "Conf Collect" v1 { collect "events" max 50 timeout 300s }"#,
        ),
        (
            WorkflowNodeKind::Barrier,
            r#"workflow "Conf Barrier" v1 { barrier "shard-sync" count 4 timeout 600s }"#,
        ),
        (
            WorkflowNodeKind::CircuitBreaker,
            r#"workflow "Conf Breaker" v1 { circuit_breaker "payment-api" threshold 5 window 60s cooldown 120s }"#,
        ),
        (
            WorkflowNodeKind::EventSource,
            r#"workflow "Conf Events" v1 { event_source type "file.uploaded" max 100 timeout 3600s }"#,
        ),
        (
            WorkflowNodeKind::Config,
            r#"
            workflow "Conf Config" v1 {
                params { ticket: string }
                set name = "renamed: ${params.ticket}"
            }"#,
        ),
        (
            WorkflowNodeKind::Input,
            r#"workflow "Conf Input" v1 { input "how many shards?" }"#,
        ),
        (
            WorkflowNodeKind::Output,
            r#"
            workflow "Conf Output" v1 {
                params { amount: int }
                output {
                    emit "ready" { value: params.amount }
                }
            }"#,
        ),
        (
            WorkflowNodeKind::Fail,
            r#"workflow "Conf Fail" v1 { fail "boom" }"#,
        ),
    ]
}

fn kinds_in(definition: &WorkflowDefinition) -> Vec<WorkflowNodeKind> {
    definition
        .definition
        .nodes
        .iter()
        .map(|node| node.kind.clone())
        .collect()
}

/// every kind in the model is either exercised by a fixture or explicitly declared surface-less.
#[test]
fn every_node_kind_is_accounted_for() {
    let fixtures = fixtures();
    for kind in WorkflowNodeKind::ALL {
        let has_fixture = fixtures.iter().any(|(fixture, _)| *fixture == kind);
        let excused = NO_REXRAP_SURFACE
            .iter()
            .any(|(excused, _)| *excused == kind);
        assert!(
            has_fixture ^ excused,
            "{kind:?} needs exactly one of: a rexrap fixture in `fixtures()`, or an entry in \
             `NO_REXRAP_SURFACE` explaining why it has no surface syntax"
        );
    }
}

/// each fixture compiles to a graph that actually contains the kind it claims to cover.
#[test]
fn every_fixture_produces_its_node_kind() {
    for (kind, src) in fixtures() {
        let definition = compile(&kind, src)
            .unwrap_or_else(|err| panic!("{kind:?} fixture failed to compile: {err}\n{src}"));
        let produced = kinds_in(&definition);
        assert!(
            produced.contains(&kind),
            "{kind:?} fixture compiled but produced no node of that kind (got {produced:?})\n{src}"
        );
    }
}

/// each fixture survives compile -> decompile -> compile with an identical normalized graph.
///
/// this is the guard on the round-trip contract: a kind whose decompiler drops a parameter fails
/// here even though every `match` over `WorkflowNodeKind` still compiles.
#[test]
fn every_node_kind_round_trips_through_rexrap() {
    for (kind, src) in fixtures() {
        let first = compile(&kind, src)
            .unwrap_or_else(|err| panic!("{kind:?} fixture failed to compile: {err}\n{src}"));
        let rexrap = decompile(&first)
            .unwrap_or_else(|err| panic!("{kind:?} failed to decompile: {err}\n{src}"));
        let second = compile(&kind, &rexrap).unwrap_or_else(|err| {
            panic!("{kind:?} failed to recompile\n{err}\n--- decompiled ---\n{rexrap}")
        });

        let normalize = |definition: WorkflowDefinition| {
            runinator_workflows::normalize_definition(definition.definition)
        };
        assert_eq!(
            normalize(first),
            normalize(second),
            "{kind:?} round trip diverged\n--- source ---\n{src}\n--- decompiled ---\n{rexrap}"
        );
    }
}

/// decompiled output for every kind is already formatted.
///
/// the editor regenerates its rexrap pane via decompile, so a kind whose decompiler emits
/// non-canonical text makes the Format button silently rewrite the document on save.
#[test]
fn every_node_kind_decompiles_to_formatted_rexrap() {
    for (kind, src) in fixtures() {
        let definition = compile(&kind, src)
            .unwrap_or_else(|err| panic!("{kind:?} fixture failed to compile: {err}\n{src}"));
        let decompiled = decompile(&definition)
            .unwrap_or_else(|err| panic!("{kind:?} failed to decompile: {err}"));
        let formatted = crate::format_str(&decompiled)
            .unwrap_or_else(|err| panic!("{kind:?} decompiled output does not format: {err}"));
        assert_eq!(
            decompiled, formatted,
            "{kind:?} decompiled output is not format-stable\n--- decompiled ---\n{decompiled}\n--- formatted ---\n{formatted}"
        );
    }
}
