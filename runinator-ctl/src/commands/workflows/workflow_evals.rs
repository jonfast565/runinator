//! live agent evaluations: corpus loading, durable case runs, and deterministic or judge scoring.

use super::*;
use runinator_models::workflow_vm::WorkflowJournalEntry;
use serde::{Deserialize, Serialize};

fn default_judge_pointer() -> String {
    "/pass".into()
}

pub(super) async fn workflows_eval(
    client: &Client,
    file: &Path,
    corpus: &Path,
    filter: Option<&str>,
    timeout: Duration,
    json_output: bool,
) -> Result<()> {
    let compiled = pack::load_workflow_bundle(file).map_err(|error| err(error.to_string()))?;
    if compiled.workflows.is_empty() {
        return Err(err(format!(
            "no workflows compiled from {}",
            file.display()
        )));
    }
    let cases = load_eval_cases(corpus, filter)?;
    if cases.is_empty() {
        return Err(err("no evaluation cases matched"));
    }

    // Applying is deliberate: unlike offline workflow tests, evals exercise the real providers and
    // must use the exact compiled revision whose prompt manifests are reported below.
    apply_workflow_source(client, file, false, None).await?;
    let eval_id = Uuid::now_v7();
    let mut reports = Vec::with_capacity(cases.len());
    for (suite_workflow, case) in cases {
        let target = case
            .workflow
            .as_deref()
            .or(suite_workflow.as_deref())
            .map(str::to_string)
            .or_else(|| match compiled.workflows.as_slice() {
                [single] => Some(single.name.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                err(format!(
                    "evaluation case '{}' must name a workflow because the pack has several",
                    case.name
                ))
            })?;
        let definition = fetch_workflow_ref(client, &target).await?;
        let workflow_id = definition
            .id
            .ok_or_else(|| err(format!("workflow '{target}' has no persisted id")))?;
        let run = client
            .create_workflow_run_with_options(
                workflow_id,
                case.input.clone(),
                false,
                Some(format!("eval:{eval_id}:{}", case.name)),
            )
            .await?;
        let (terminal, output) = wait_for_eval_output(client, run.id, timeout).await?;
        let (passed, failure, judge_run_id) =
            if terminal.status.is_terminal() && terminal.status == WorkflowStatus::Succeeded {
                score_case(client, eval_id, &case, &output, timeout).await?
            } else {
                (
                    false,
                    Some(format!(
                        "workflow ended {}: {}",
                        terminal.status.as_str(),
                        terminal.message.unwrap_or_else(|| "no message".into())
                    )),
                    None,
                )
            };
        let prompt_assets = definition
            .definition
            .metadata
            .get("prompt_assets")
            .cloned()
            .unwrap_or_default();
        reports.push(EvalCaseReport {
            name: case.name,
            workflow: target,
            run_id: run.id,
            status: terminal.status.as_str().into(),
            passed,
            failure,
            judge_run_id,
            prompt_assets,
        });
    }

    let passed = reports.iter().filter(|report| report.passed).count();
    let report = EvalReport {
        eval_id,
        pack: file.display().to_string(),
        total: reports.len(),
        passed,
        failed: reports.len() - passed,
        agreement_rate: passed as f64 / reports.len() as f64,
        cases: reports,
    };
    if json_output {
        output::json(&report)?;
    } else {
        print_eval_report(&report);
    }
    if report.failed > 0 {
        return Err(err(format!(
            "{} evaluation case(s) disagreed; eval {}",
            report.failed, report.eval_id
        )));
    }
    Ok(())
}

fn load_eval_cases(corpus: &Path, filter: Option<&str>) -> Result<Vec<(Option<String>, EvalCase)>> {
    let mut paths = if corpus.is_dir() {
        fs::read_dir(corpus)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension().and_then(|extension| extension.to_str()) == Some("json")
            })
            .collect::<Vec<_>>()
    } else {
        vec![corpus.to_path_buf()]
    };
    paths.sort();
    let mut cases = Vec::new();
    for path in paths {
        let bytes = fs::read(&path)?;
        let suite: EvalSuite = serde_json::from_slice(&bytes).map_err(|error| {
            err(format!(
                "parse evaluation corpus {}: {error}",
                path.display()
            ))
        })?;
        for case in suite.cases {
            if filter.is_some_and(|needle| !case.name.contains(needle)) {
                continue;
            }
            if case.expect.is_none() && case.judge.is_none() {
                return Err(err(format!(
                    "evaluation case '{}' in {} needs expect or judge",
                    case.name,
                    path.display()
                )));
            }
            cases.push((suite.workflow.clone(), case));
        }
    }
    Ok(cases)
}

async fn wait_for_eval_output(
    client: &Client,
    run_id: Uuid,
    timeout: Duration,
) -> Result<(WorkflowRun, Value)> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let run = client.fetch_workflow_run(run_id).await?;
        if run.status.is_terminal() {
            let output = client
                .fetch_workflow_journal(run_id)
                .await?
                .into_iter()
                .rev()
                .find_map(|record| match record.entry {
                    WorkflowJournalEntry::Completed { value, .. } => Some(value),
                    _ => None,
                })
                .unwrap_or_default();
            return Ok((run, output));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(err(format!("evaluation run {run_id} exceeded its timeout")));
        }
        time::sleep(Duration::from_secs(1)).await;
    }
}

async fn score_case(
    client: &Client,
    eval_id: Uuid,
    case: &EvalCase,
    candidate: &Value,
    timeout: Duration,
) -> Result<(bool, Option<String>, Option<Uuid>)> {
    if let Some(expectation) = &case.expect {
        let actual = candidate.pointer(&expectation.pointer);
        let passed = actual == Some(&expectation.equals);
        let failure = (!passed).then(|| {
            format!(
                "{} expected {}, got {}",
                if expectation.pointer.is_empty() {
                    "result"
                } else {
                    expectation.pointer.as_str()
                },
                expectation.equals,
                actual.cloned().unwrap_or_default()
            )
        });
        return Ok((passed, failure, None));
    }
    let Some(judge) = &case.judge else {
        return Ok((false, Some("case has no scorer".into()), None));
    };
    let definition = fetch_workflow_ref(client, &judge.workflow).await?;
    let workflow_id = definition.id.ok_or_else(|| {
        err(format!(
            "judge workflow '{}' has no persisted id",
            judge.workflow
        ))
    })?;
    let mut parameters = judge.input.clone();
    if !parameters.is_object() {
        parameters = runinator_models::json!({ "rubric": parameters });
    }
    if let Some(object) = parameters.as_object_mut() {
        object.insert("candidate".into(), candidate.clone());
    }
    let run = client
        .create_workflow_run_with_options(
            workflow_id,
            parameters,
            false,
            Some(format!("eval:{eval_id}:judge:{}", case.name)),
        )
        .await?;
    let (terminal, output) = wait_for_eval_output(client, run.id, timeout).await?;
    let passed = terminal.status == WorkflowStatus::Succeeded
        && output.pointer(&judge.pass_pointer).and_then(Value::as_bool) == Some(true);
    let failure = (!passed).then(|| {
        format!(
            "judge '{}' did not return true at {}",
            judge.workflow, judge.pass_pointer
        )
    });
    Ok((passed, failure, Some(run.id)))
}

fn print_eval_report(report: &EvalReport) {
    for case in &report.cases {
        println!(
            "{}  [{}] {}  run={}",
            if case.passed { "PASS" } else { "FAIL" },
            case.workflow,
            case.name,
            case.run_id
        );
        if let Some(failure) = &case.failure {
            println!("      - {failure}");
        }
    }
    println!();
    println!(
        "{} passed, {} failed, {} total; agreement {:.1}% (eval {})",
        report.passed,
        report.failed,
        report.total,
        report.agreement_rate * 100.0,
        report.eval_id
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_filters_a_corpus() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("review.json"),
            r#"{"workflow":"Review","cases":[{"name":"approve","input":{},"expect":{"pointer":"/next","equals":"merge"}},{"name":"repair","input":{},"expect":{"pointer":"/next","equals":"repair"}}]}"#,
        )
        .unwrap();
        let cases = load_eval_cases(dir.path(), Some("approve")).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].0.as_deref(), Some("Review"));
        assert_eq!(cases[0].1.name, "approve");
    }
}

mod eval_suite;
use eval_suite::EvalSuite;

mod eval_case;
use eval_case::EvalCase;

mod eval_expectation;
use eval_expectation::EvalExpectation;

mod eval_judge;
use eval_judge::EvalJudge;

mod eval_case_report;
use eval_case_report::EvalCaseReport;

mod eval_report;
use eval_report::EvalReport;
