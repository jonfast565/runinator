use super::*;

pub fn workflows_test(
    file: &Path,
    tests: &[PathBuf],
    filter: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let bundle = pack::load_workflow_bundle(file).map_err(|e| err(e.to_string()))?;
    if bundle.workflows.is_empty() {
        return Err(err(format!(
            "no workflows compiled from {}",
            file.display()
        )));
    }

    let suite_files = if tests.is_empty() {
        discover_rexrapt_suites(file)?
    } else {
        tests.to_vec()
    };
    if suite_files.is_empty() {
        return Err(err(
            "no tests blocks found; pass one or more .rrx sources with --tests <path>",
        ));
    }

    let mut reports: Vec<CaseReport> = Vec::new();
    for suite_path in &suite_files {
        let data = fs::read_to_string(suite_path)
            .map_err(|e| err(format!("read {}: {e}", suite_path.display())))?;
        let blocks = runinator_rexrap::parse_rrx_blocks(&data)
            .map_err(|e| err(format!("parse {}: {}", suite_path.display(), e.render(&data))))?;
        for test_source in blocks.tests {
            let suite: runinator_workflows::WorkflowTestSuite = serde_json::from_str(&test_source)
                .map_err(|e| err(format!("parse tests in {}: {e}", suite_path.display())))?;
            for case in &suite.tests {
                if let Some(needle) = filter
                    && !case.name.contains(needle)
                {
                    continue;
                }
                let target = case.workflow.as_deref().or(suite.workflow.as_deref());
                let definition = select_test_workflow(&bundle.workflows, target)?;
                let result = runinator_workflows::run_test_case(definition, case);
                reports.push(CaseReport {
                    suite: suite_path.display().to_string(),
                    workflow: definition.name.clone(),
                    result,
                });
            }
        }
    }

    if reports.is_empty() {
        return Err(err("no test cases matched"));
    }

    if json_output {
        let cases: Vec<Value> = reports
            .iter()
            .map(|report| {
                json!({
                    "suite": report.suite,
                    "workflow": report.workflow,
                    "name": report.result.name,
                    "passed": report.result.passed,
                    "failures": report.result.failures,
                    "status": report.result.run.status.as_str(),
                })
            })
            .collect();
        let passed = reports.iter().filter(|r| r.result.passed).count();
        output::json(&json!({
            "total": reports.len(),
            "passed": passed,
            "failed": reports.len() - passed,
            "cases": cases,
        }))?;
    } else {
        print_test_reports(&reports);
    }

    let failed = reports.iter().filter(|r| !r.result.passed).count();
    if failed > 0 {
        return Err(err(format!("{failed} test case(s) failed")));
    }
    Ok(())
}

struct CaseReport {
    suite: String,
    workflow: String,
    result: runinator_workflows::TestCaseResult,
}

// pick the workflow a case targets: by name when given, else the pack's sole workflow.
fn select_test_workflow<'a>(
    workflows: &'a [WorkflowDefinition],
    target: Option<&str>,
) -> Result<&'a WorkflowDefinition> {
    match target {
        Some(name) => workflows
            .iter()
            .find(|workflow| workflow.name == name)
            .ok_or_else(|| err(format!("test references unknown workflow '{name}'"))),
        None => match workflows {
            [single] => Ok(single),
            _ => Err(err(
                "pack defines multiple workflows; set \"workflow\" on the suite or case",
            )),
        },
    }
}

// Every `.rrx` source can carry `tests { ... }`, so discover the same source set the pack compiler
// reads rather than looking for a second extension.
fn discover_rexrapt_suites(file: &Path) -> Result<Vec<PathBuf>> {
    let mut suites = Vec::new();
    if file.is_dir() {
        for entry in fs::read_dir(file).map_err(|e| err(e.to_string()))? {
            let path = entry.map_err(|e| err(e.to_string()))?.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("rrx") {
                suites.push(path);
            }
        }
    } else if file.extension().and_then(|ext| ext.to_str()) == Some("rrx") {
        suites.push(file.to_path_buf());
    }
    suites.sort();
    Ok(suites)
}

fn print_test_reports(reports: &[CaseReport]) {
    let passed = reports.iter().filter(|r| r.result.passed).count();
    for report in reports {
        let mark = if report.result.passed { "PASS" } else { "FAIL" };
        println!("{mark}  [{}] {}", report.workflow, report.result.name);
        for failure in &report.result.failures {
            println!("        - {failure}");
        }
        if let Some(error) = &report.result.run.error {
            println!("        ! {error}");
        }
    }
    println!();
    println!(
        "{} passed, {} failed, {} total",
        passed,
        reports.len() - passed,
        reports.len()
    );
}
