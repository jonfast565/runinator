use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;
use runinator_models::types::RuninatorType;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::Provider;
use std::{fs, process::Command};
use uuid::Uuid;

use crate::StdProvider;
use crate::code::{parse_code_output, validate_code_output};
use crate::foreign_languages::adapter_for;

const PYTHON_RETURN_SOURCE: &str = r#"def main(context):
    return {"answer": context["input"]["value"] + 1}
"#;

const ASYNC_PYTHON_RETURN_SOURCE: &str = r#"async def main(context):
    return {"answer": context["input"]["value"] + 1}
"#;

fn request_for(
    action_function: &str,
    parameters: runinator_models::value::Value,
) -> ProviderExecutionRequest {
    ProviderExecutionRequest {
        run_id: Some(Uuid::now_v7()),
        action_name: "std".into(),
        action_function: action_function.into(),
        parameters,
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
    }
}

#[test]
fn code_rejects_missing_language_before_docker() {
    let provider = StdProvider;
    let parameters = json!({
        "source": "print({})",
        "context": {}
    });
    let err = provider
        .execute_service(
            request_for("code", parameters),
            None,
            CancellationToken::new(),
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("missing string parameter 'language'")
    );
}

#[test]
fn code_rejects_unsupported_language_before_docker() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "lua",
        "source": "print({})",
        "runtime": { "image": "lua:latest", "setup_script": "" },
        "context": {}
    });
    let err = provider
        .execute_service(
            request_for("code", parameters),
            None,
            CancellationToken::new(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("supported languages"));
}

#[test]
fn code_rejects_missing_runtime_before_docker() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "python",
        "source": "print({})",
        "context": {}
    });
    let err = provider
        .execute_service(
            request_for("code", parameters),
            None,
            CancellationToken::new(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("missing runtime config"));
}

#[test]
fn code_language_specs_support_restored_languages_and_aliases() {
    let cases = [
        (
            "python",
            "python",
            "foreign.py",
            "python /work/runinator_runner.py",
        ),
        (
            "py",
            "python",
            "foreign.py",
            "python /work/runinator_runner.py",
        ),
        (
            "javascript",
            "javascript",
            "foreign.mjs",
            "node /work/runinator_runner.mjs",
        ),
        (
            "js",
            "javascript",
            "foreign.mjs",
            "node /work/runinator_runner.mjs",
        ),
        (
            "node",
            "javascript",
            "foreign.mjs",
            "node /work/runinator_runner.mjs",
        ),
        (
            "bash",
            "bash",
            "foreign.sh",
            "bash /work/runinator_runner.sh",
        ),
        ("sh", "bash", "foreign.sh", "bash /work/runinator_runner.sh"),
        (
            "ruby",
            "ruby",
            "foreign.rb",
            "ruby /work/runinator_runner.rb",
        ),
        ("rb", "ruby", "foreign.rb", "ruby /work/runinator_runner.rb"),
        (
            "perl",
            "perl",
            "foreign.pl",
            "perl /work/runinator_runner.pl",
        ),
        ("pl", "perl", "foreign.pl", "perl /work/runinator_runner.pl"),
        (
            "php",
            "php",
            "foreign.php",
            "php /work/runinator_runner.php",
        ),
        (
            "go",
            "go",
            "foreign.go",
            "go run /work/runinator_runner.go /work/foreign.go",
        ),
        (
            "golang",
            "go",
            "foreign.go",
            "go run /work/runinator_runner.go /work/foreign.go",
        ),
        (
            "swift",
            "swift",
            "foreign.swift",
            "swiftc -module-cache-path /tmp/runinator-module-cache /work/foreign.swift /work/main.swift -o /tmp/runinator_foreign && /tmp/runinator_foreign",
        ),
        (
            "powershell",
            "powershell",
            "foreign.ps1",
            "pwsh -NoLogo -NoProfile -NonInteractive -File /work/runinator_runner.ps1",
        ),
        (
            "pwsh",
            "powershell",
            "foreign.ps1",
            "pwsh -NoLogo -NoProfile -NonInteractive -File /work/runinator_runner.ps1",
        ),
        (
            "ps1",
            "powershell",
            "foreign.ps1",
            "pwsh -NoLogo -NoProfile -NonInteractive -File /work/runinator_runner.ps1",
        ),
        (
            "csharp",
            "csharp",
            "Foreign.cs",
            "dotnet run --project /work/runinator.csproj --configuration Release --artifacts-path /tmp/runinator-csharp-artifacts",
        ),
        (
            "c#",
            "csharp",
            "Foreign.cs",
            "dotnet run --project /work/runinator.csproj --configuration Release --artifacts-path /tmp/runinator-csharp-artifacts",
        ),
        (
            "fsharp",
            "fsharp",
            "Foreign.fs",
            "dotnet run --project /work/runinator.fsproj --configuration Release --artifacts-path /tmp/runinator-fsharp-artifacts",
        ),
        (
            "f#",
            "fsharp",
            "Foreign.fs",
            "dotnet run --project /work/runinator.fsproj --configuration Release --artifacts-path /tmp/runinator-fsharp-artifacts",
        ),
        (
            "vbnet",
            "vbnet",
            "Foreign.vb",
            "dotnet run --project /work/runinator.vbproj --configuration Release --artifacts-path /tmp/runinator-vbnet-artifacts",
        ),
        (
            "vb.net",
            "vbnet",
            "Foreign.vb",
            "dotnet run --project /work/runinator.vbproj --configuration Release --artifacts-path /tmp/runinator-vbnet-artifacts",
        ),
    ];

    for (input, canonical, filename, command) in cases {
        let adapter = adapter_for(input).expect(input);
        assert_eq!(adapter.canonical(), canonical, "{input}");
        assert_eq!(adapter.source_filename(), filename, "{input}");
        assert_eq!(adapter.execute(), command, "{input}");
        assert!(
            adapter.runner_source().to_lowercase().contains("main"),
            "{input}"
        );
    }
}

#[test]
fn code_output_reads_the_runner_file() {
    let dir = std::env::temp_dir().join(format!("runinator-code-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let output_path = dir.join("output.json");
    fs::write(&output_path, r#"{"value":42}"#).unwrap();

    let value = parse_code_output(&output_path).unwrap();

    assert_eq!(value, json!({ "value": 42 }));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn code_output_does_not_fall_back_to_stdout() {
    let output_path = std::env::temp_dir().join(format!("missing-{}.json", Uuid::new_v4()));
    let err = parse_code_output(&output_path).unwrap_err();
    assert!(err.to_string().contains("did not return JSON"));
}

#[test]
fn python_output_contract_returns_json() {
    for source in [PYTHON_RETURN_SOURCE, ASYNC_PYTHON_RETURN_SOURCE] {
        assert_eq!(run_python_contract(source), json!({ "answer": 42 }));
    }
}

#[test]
fn python_output_contract_requires_main() {
    let dir = std::env::temp_dir().join(format!("runinator-python-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let adapter = adapter_for("python").unwrap();
    let runner_path = dir.join(adapter.runner_filename());
    let context_path = dir.join("context.json");
    let output_path = dir.join("output.json");
    fs::write(dir.join(adapter.source_filename()), "value = 42\n").unwrap();
    fs::write(&runner_path, adapter.runner_source()).unwrap();
    fs::write(&context_path, "{}").unwrap();

    let process = Command::new("python3")
        .arg(&runner_path)
        .env("RUNINATOR_CONTEXT", &context_path)
        .env("RUNINATOR_OUTPUT", &output_path)
        .output()
        .expect("python3");

    assert!(!process.status.success());
    assert!(
        String::from_utf8_lossy(&process.stderr).contains("foreign code must define main(context)")
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn installed_language_runners_return_json() {
    let cases = [
        (
            "javascript",
            "node",
            "export function main(context) { return { answer: context.input.value + 1 }; }\n",
        ),
        (
            "bash",
            "bash",
            "main() { printf '{\"answer\":%s}' \"$(( $(printf '%s' \"$1\" | sed -E 's/.*\"value\":([0-9]+).*/\\1/') + 1 ))\"; }\n",
        ),
        (
            "ruby",
            "ruby",
            "def main(context)\n  { \"answer\" => context[\"input\"][\"value\"] + 1 }\nend\n",
        ),
        (
            "perl",
            "perl",
            "sub main { my ($context) = @_; return { answer => $context->{input}{value} + 1 }; }\n1;\n",
        ),
        (
            "php",
            "php",
            "<?php\nfunction main(array $context): array {\n    return [\"answer\" => $context[\"input\"][\"value\"] + 1];\n}\n",
        ),
    ];

    for (language, executable, source) in cases {
        assert_eq!(
            run_installed_language_contract(language, executable, source),
            json!({ "answer": 42 }),
            "{language}"
        );
    }
}

#[test]
fn installed_compiled_and_shell_runners_return_json() {
    let cases = [
        (
            "go",
            "go",
            "version",
            r#"package main

func Main(context any) any {
	input := context.(map[string]any)["input"].(map[string]any)
	return map[string]any{"answer": input["value"].(float64) + 1}
}
"#,
        ),
        (
            "swift",
            "swiftc",
            "--version",
            r#"import Foundation

func main(_ context: Any) throws -> Any {
    let input = (context as! [String: Any])["input"] as! [String: Any]
    return ["answer": (input["value"] as! NSNumber).intValue + 1]
}
"#,
        ),
        (
            "powershell",
            "pwsh",
            "--version",
            "function main($context) { return @{ answer = $context.input.value + 1 } }\n",
        ),
        (
            "csharp",
            "dotnet",
            "--version",
            r#"using System.Text.Json;

public static class Foreign
{
    public static object Main(JsonElement context) => new
    {
        answer = context.GetProperty("input").GetProperty("value").GetInt32() + 1
    };
}
"#,
        ),
        (
            "fsharp",
            "dotnet",
            "--version",
            r#"module Foreign

open System.Text.Json

let main (context: JsonElement) : obj =
    let answer = context.GetProperty("input").GetProperty("value").GetInt32() + 1
    box {| answer = answer |}
"#,
        ),
        (
            "vbnet",
            "dotnet",
            "--version",
            r#"Imports System.Text.Json

Public Module Foreign
    Public Function Main(context As JsonElement) As Object
        Return New With {.answer = context.GetProperty("input").GetProperty("value").GetInt32() + 1}
    End Function
End Module
"#,
        ),
    ];

    for (language, executable, version_arg, source) in cases {
        if Command::new(executable).arg(version_arg).output().is_err() {
            continue;
        }
        assert_eq!(
            run_command_language_contract(language, source),
            json!({ "answer": 42 }),
            "{language}"
        );
    }
}

fn run_command_language_contract(language: &str, source: &str) -> runinator_models::value::Value {
    let dir = std::env::temp_dir().join(format!("runinator-{language}-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let adapter = adapter_for(language).unwrap();
    let context_path = dir.join("context.json");
    let output_path = dir.join("output.json");
    fs::write(dir.join(adapter.source_filename()), source).unwrap();
    fs::write(dir.join(adapter.runner_filename()), adapter.runner_source()).unwrap();
    for (filename, contents) in adapter.additional_files() {
        fs::write(dir.join(filename), contents).unwrap();
    }
    fs::write(&context_path, r#"{"input":{"value":41}}"#).unwrap();

    let command = adapter.execute().replace("/work", &dir.to_string_lossy());
    let process = Command::new("bash")
        .args(["-c", &command])
        .env("RUNINATOR_CONTEXT", &context_path)
        .env("RUNINATOR_OUTPUT", &output_path)
        .env("SWIFT_MODULECACHE_PATH", dir.join("swift-module-cache"))
        .output()
        .unwrap_or_else(|err| panic!("failed to start {language}: {err}"));
    assert!(
        process.status.success(),
        "{language} failed: {}",
        String::from_utf8_lossy(&process.stderr)
    );

    let value = parse_code_output(&output_path).unwrap();
    let _ = fs::remove_dir_all(dir);
    value
}

fn run_installed_language_contract(
    language: &str,
    executable: &str,
    source: &str,
) -> runinator_models::value::Value {
    let dir = std::env::temp_dir().join(format!("runinator-{language}-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let adapter = adapter_for(language).unwrap();
    let runner_path = dir.join(adapter.runner_filename());
    let context_path = dir.join("context.json");
    let output_path = dir.join("output.json");
    fs::write(dir.join(adapter.source_filename()), source).unwrap();
    fs::write(&runner_path, adapter.runner_source()).unwrap();
    fs::write(&context_path, r#"{"input":{"value":41}}"#).unwrap();

    let process = Command::new(executable)
        .arg(&runner_path)
        .env("RUNINATOR_CONTEXT", &context_path)
        .env("RUNINATOR_OUTPUT", &output_path)
        .output()
        .unwrap_or_else(|err| panic!("failed to start {language}: {err}"));
    assert!(
        process.status.success(),
        "{language} failed: {}",
        String::from_utf8_lossy(&process.stderr)
    );

    let value = parse_code_output(&output_path).unwrap();
    let _ = fs::remove_dir_all(dir);
    value
}

fn run_python_contract(source: &str) -> runinator_models::value::Value {
    let dir = std::env::temp_dir().join(format!("runinator-python-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let adapter = adapter_for("python").unwrap();
    let source_path = dir.join(adapter.source_filename());
    let runner_path = dir.join(adapter.runner_filename());
    let context_path = dir.join("context.json");
    let output_path = dir.join("output.json");
    fs::write(&source_path, source).unwrap();
    fs::write(&runner_path, adapter.runner_source()).unwrap();
    fs::write(&context_path, r#"{"input":{"value":41}}"#).unwrap();

    let process = Command::new("python3")
        .arg(&runner_path)
        .env("RUNINATOR_CONTEXT", &context_path)
        .env("RUNINATOR_OUTPUT", &output_path)
        .output()
        .expect("python3 must be available for the foreign-compute contract test");
    assert!(
        process.status.success(),
        "python failed: {}",
        String::from_utf8_lossy(&process.stderr)
    );

    let value = parse_code_output(&output_path).unwrap();
    let _ = fs::remove_dir_all(dir);
    value
}

#[test]
fn declared_foreign_output_type_is_enforced() {
    let expected = RuninatorType::structure([("answer", RuninatorType::Integer)]);
    validate_code_output(&json!({ "answer": 42 }), Some(&expected)).unwrap();

    let err = validate_code_output(&json!({ "answer": "forty-two" }), Some(&expected)).unwrap_err();
    assert!(
        err.to_string()
            .contains("foreign compute result.answer expected integer, got string"),
        "{err}"
    );
}

#[test]
#[ignore = "requires a running Docker daemon and the python:3.12 image"]
fn python_foreign_compute_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "python",
        "source": PYTHON_RETURN_SOURCE,
        "runtime": { "image": "python:3.12", "setup_script": "" },
        "context": { "input": { "value": 41 } }
    });

    let result = provider
        .execute_service(
            request_for("code", parameters),
            None,
            CancellationToken::new(),
        )
        .expect("python foreign compute");

    assert_eq!(result.output_json, Some(json!({ "answer": 42 })));
}

#[test]
fn metadata_advertises_code() {
    let metadata = StdProvider.metadata();
    let code = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "code")
        .unwrap();
    assert!(!code.pure);
    assert!(
        code.parameters
            .iter()
            .any(|parameter| parameter.name == "context" && !parameter.required)
    );
    assert!(
        code.parameters
            .iter()
            .any(|parameter| parameter.name == "runtime" && !parameter.required)
    );
}
