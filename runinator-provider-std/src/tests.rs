use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;
use runinator_models::types::RuninatorType;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::Provider;
use std::{fs, process::Command};
use uuid::Uuid;

use crate::StdProvider;
use crate::code::{parse_code_output, validate_code_output};
use crate::foreign_languages::{ToolchainConfig, adapter_for};

const PYTHON_RETURN_SOURCE: &str = r#"def main(context):
    return {"answer": context["input"]["value"] + 1}
"#;

const ASYNC_PYTHON_RETURN_SOURCE: &str = r#"async def main(context):
    return {"answer": context["input"]["value"] + 1}
"#;

const COMMON_LISP_RETURN_SOURCE: &str = r#"(defun main (context)
  (let* ((input (gethash "input" context))
         (result (make-hash-table :test #'equal)))
    (setf (gethash "answer" result) (1+ (gethash "value" input)))
    result))
"#;

const COBOL_RETURN_SOURCE: &str = r#"identification division.
program-id. runinator-compute.
data division.
working-storage section.
01 context-json pic x(4096).
procedure division.
    accept context-json
    display '{"answer":42}'
    stop run.
"#;

const C_RETURN_SOURCE: &str = r#"#include <stdio.h>
#include <string.h>

int main(void) {
    char context[4096];
    if (fgets(context, sizeof context, stdin) == NULL || strstr(context, "\"value\":41") == NULL) {
        fputs("context JSON was not provided on stdin\n", stderr);
        return 1;
    }
    fputs("{\"answer\":42}", stdout);
    return 0;
}
"#;

const CPP_RETURN_SOURCE: &str = r#"#include <iostream>
#include <string>

int main() {
    std::string context;
    std::getline(std::cin, context);
    if (context.find("\"value\":41") == std::string::npos) {
        std::cerr << "context JSON was not provided on stdin\n";
        return 1;
    }
    std::cout << "{\"answer\":42}";
    return 0;
}
"#;

const FORTRAN_RETURN_SOURCE: &str = r#"program runinator_foreign
    use, intrinsic :: iso_fortran_env, only: error_unit
    implicit none
    character(len=4096) :: context
    integer :: status

    read (*, '(A)', iostat=status) context
    if (status /= 0 .or. index(context, '"value":41') == 0) then
        write (error_unit, '(A)') 'context JSON was not provided on stdin'
        error stop 1
    end if
    write (*, '(A)', advance='no') '{"answer":42}'
end program runinator_foreign
"#;

const ADA_RETURN_SOURCE: &str = r#"with Ada.Command_Line; use Ada.Command_Line;
with Ada.Strings.Fixed;
with Ada.Text_IO; use Ada.Text_IO;

procedure Runinator_Foreign is
   Context : String (1 .. 4096);
   Last    : Natural;
begin
   Get_Line (Context, Last);
   if Last = 0 or else Ada.Strings.Fixed.Index (Context (1 .. Last), """value"":41") = 0 then
      Put_Line (Standard_Error, "context JSON was not provided on stdin");
      Set_Exit_Status (Failure);
      return;
   end if;
   Put ("{""answer"":42}");
end Runinator_Foreign;
"#;

const HASKELL_RETURN_SOURCE: &str = r#"{-# LANGUAGE OverloadedStrings #-}

module Foreign (runinatorMain) where

import Data.Aeson (Value, object, withObject, (.:), (.=))
import Data.Aeson.Types (parseEither)

runinatorMain :: Value -> Value
runinatorMain context =
    case parseEither parser context of
        Left message -> error message
        Right value -> object ["answer" .= (value + (1 :: Int))]
  where
    parser = withObject "context" $ \root -> do
        input <- root .: "input"
        input .: "value"
"#;

const HASKELL_IO_RETURN_SOURCE: &str = r#"{-# LANGUAGE OverloadedStrings #-}

module Foreign (runinatorMain) where

import Data.Aeson (Value, object, withObject, (.:), (.=))
import Data.Aeson.Types (parseEither)

runinatorMain :: Value -> IO Value
runinatorMain context = pure $ case parseEither parser context of
    Left message -> error message
    Right value -> object ["answer" .= (value + (1 :: Int))]
  where
    parser = withObject "context" $ \root -> do
        input <- root .: "input"
        input .: "value"
"#;

const OCAML_RETURN_SOURCE: &str = r#"let runinator_main (context : Yojson.Safe.t) : Yojson.Safe.t =
  let open Yojson.Safe.Util in
  let value = context |> member "input" |> member "value" |> to_int in
  `Assoc [("answer", `Int (value + 1))]
"#;

const ERLANG_RETURN_SOURCE: &str = r#"-module(foreign).
-export([runinator_main/1]).

runinator_main(Context) ->
    Input = maps:get(<<"input">>, Context),
    Value = maps:get(<<"value">>, Input),
    #{<<"answer">> => Value + 1}.
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
        workspace_path: None,
        execution_profile: None,
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
            "commonlisp",
            "commonlisp",
            "foreign.lisp",
            "sbcl --noinform --disable-debugger --script /work/runinator_runner.lisp",
        ),
        (
            "common-lisp",
            "commonlisp",
            "foreign.lisp",
            "sbcl --noinform --disable-debugger --script /work/runinator_runner.lisp",
        ),
        (
            "lisp",
            "commonlisp",
            "foreign.lisp",
            "sbcl --noinform --disable-debugger --script /work/runinator_runner.lisp",
        ),
        (
            "cobol",
            "cobol",
            "foreign.cob",
            "bash /work/runinator_runner.sh",
        ),
        (
            "gnucobol",
            "cobol",
            "foreign.cob",
            "bash /work/runinator_runner.sh",
        ),
        ("c", "c", "foreign.c", "bash /work/runinator_runner.sh"),
        ("gcc", "c", "foreign.c", "bash /work/runinator_runner.sh"),
        (
            "cpp",
            "cpp",
            "foreign.cpp",
            "bash /work/runinator_runner.sh",
        ),
        (
            "g++",
            "cpp",
            "foreign.cpp",
            "bash /work/runinator_runner.sh",
        ),
        (
            "fortran",
            "fortran",
            "foreign.f90",
            "bash /work/runinator_runner.sh",
        ),
        (
            "gfortran",
            "fortran",
            "foreign.f90",
            "bash /work/runinator_runner.sh",
        ),
        (
            "ada",
            "ada",
            "runinator_foreign.adb",
            "bash /work/runinator_runner.sh",
        ),
        (
            "gnat",
            "ada",
            "runinator_foreign.adb",
            "bash /work/runinator_runner.sh",
        ),
        (
            "haskell",
            "haskell",
            "Foreign.hs",
            "bash /work/runinator_runner.sh",
        ),
        (
            "ghc",
            "haskell",
            "Foreign.hs",
            "bash /work/runinator_runner.sh",
        ),
        (
            "hs",
            "haskell",
            "Foreign.hs",
            "bash /work/runinator_runner.sh",
        ),
        (
            "ocaml",
            "ocaml",
            "foreign.ml",
            "bash /work/runinator_runner.sh",
        ),
        (
            "ocamlopt",
            "ocaml",
            "foreign.ml",
            "bash /work/runinator_runner.sh",
        ),
        (
            "ml",
            "ocaml",
            "foreign.ml",
            "bash /work/runinator_runner.sh",
        ),
        (
            "erlang",
            "erlang",
            "foreign.erl",
            "escript /work/runinator_runner.escript",
        ),
        (
            "erl",
            "erlang",
            "foreign.erl",
            "escript /work/runinator_runner.escript",
        ),
        (
            "escript",
            "erlang",
            "foreign.erl",
            "escript /work/runinator_runner.escript",
        ),
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
            "bash /work/runinator_runner.sh",
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
        assert!(!adapter.runner_source().trim().is_empty(), "{input}");
    }
}

#[test]
fn toolchain_overrides_are_quoted_and_keep_adapter_operands() {
    let python = adapter_for("python").unwrap();
    let command = python.rendered_execute(&ToolchainConfig {
        executable: "/opt/custom python".into(),
        build_args: vec!["-X".into(), "value with spaces".into()],
        run_args: vec!["argument's value".into()],
    });
    assert_eq!(
        command,
        "'/opt/custom python' '-X' 'value with spaces' /work/runinator_runner.py 'argument'\"'\"'s value'"
    );
}

#[test]
fn swift_compiler_override_stays_in_runner_and_binary_is_final_command() {
    let swift = adapter_for("swift").unwrap();
    let toolchain = ToolchainConfig {
        executable: "/opt/swift toolchain/swiftc".into(),
        build_args: vec!["-Onone".into()],
        run_args: vec!["hello world".into()],
    };
    let runner = swift.rendered_runner_source(&toolchain);
    assert!(runner.contains("'/opt/swift toolchain/swiftc' '-Onone' -module-cache-path"));
    assert!(runner.contains("/tmp/runinator_foreign \"$@\""));
    assert_eq!(
        swift.rendered_execute(&toolchain),
        "bash /work/runinator_runner.sh 'hello world'"
    );
}

#[test]
fn build_arguments_follow_tool_subcommands() {
    let toolchain = ToolchainConfig {
        executable: "/opt/dotnet".into(),
        build_args: vec!["--no-restore".into()],
        run_args: vec!["application argument".into()],
    };
    assert_eq!(
        adapter_for("csharp").unwrap().rendered_execute(&toolchain),
        "'/opt/dotnet' run '--no-restore' --project /work/runinator.csproj --configuration Release --artifacts-path /tmp/runinator-csharp-artifacts 'application argument'"
    );

    let go = adapter_for("go")
        .unwrap()
        .rendered_execute(&ToolchainConfig {
            executable: "go".into(),
            build_args: vec!["-tags=integration".into()],
            run_args: Vec::new(),
        });
    assert_eq!(
        go,
        "'go' run '-tags=integration' /work/runinator_runner.go /work/foreign.go"
    );
}

#[test]
fn code_rejects_reserved_environment_variables_before_docker() {
    let provider = StdProvider;
    let err = provider
        .execute_service(
            request_for(
                "code",
                json!({
                    "language": "python",
                    "source": "def main(context): return context",
                    "runtime": {
                        "image": "python:3.12",
                        "setup_script": "",
                        "environment": { "RUNINATOR_OUTPUT": "/tmp/stolen" }
                    },
                    "context": {}
                }),
            ),
            None,
            CancellationToken::new(),
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("reserved variable 'RUNINATOR_OUTPUT'")
    );
}

#[test]
fn haskell_adapter_supports_pure_and_io_results() {
    let adapter = adapter_for("haskell").unwrap();
    assert_eq!(adapter.runner_filename(), "runinator_runner.sh");

    let main_source = adapter
        .additional_files()
        .iter()
        .find_map(|(name, source)| (*name == "Main.hs").then_some(*source))
        .expect("Haskell adapter must provide Main.hs");
    assert!(main_source.contains("instance IntoRuninatorIO Value"));
    assert!(main_source.contains("instance IntoRuninatorIO (IO Value)"));
}

#[test]
fn ocaml_and_erlang_adapters_use_native_json_contracts() {
    let ocaml = adapter_for("ocaml").unwrap();
    assert_eq!(ocaml.runner_filename(), "runinator_runner.sh");
    let ocaml_main = ocaml
        .additional_files()
        .iter()
        .find_map(|(name, source)| (*name == "runinator_main.ml").then_some(*source))
        .expect("OCaml adapter must provide runinator_main.ml");
    assert!(ocaml_main.contains("Foreign.runinator_main context"));
    assert!(ocaml_main.contains("Yojson.Safe.to_file"));

    let erlang = adapter_for("erlang").unwrap();
    assert_eq!(erlang.runner_filename(), "runinator_runner.escript");
    assert!(erlang.runner_source().contains("jiffy:decode"));
    assert!(erlang.runner_source().contains("foreign:runinator_main"));
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
        (
            "cobol",
            "cobc",
            "--version",
            r#"identification division.
program-id. runinator-test.

data division.
working-storage section.
01 context-json pic x(4096).
01 match-count pic 9 value 0.

procedure division.
    accept context-json
    inspect context-json tallying match-count for all '"value":41'
    if match-count = 0
        display "context JSON was not provided on stdin" upon syserr
        stop run returning 1
    end-if
    display '{"answer":42}'
    stop run.
"#,
        ),
        ("c", "gcc", "--version", C_RETURN_SOURCE),
        ("cpp", "g++", "--version", CPP_RETURN_SOURCE),
        ("fortran", "gfortran", "--version", FORTRAN_RETURN_SOURCE),
        ("ada", "gnatmake", "--version", ADA_RETURN_SOURCE),
        ("haskell", "ghc", "--version", HASKELL_RETURN_SOURCE),
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

#[test]
fn installed_common_lisp_runner_returns_json_when_yason_is_available() {
    if Command::new("sbcl").arg("--version").output().is_err() {
        return;
    }
    let yason = Command::new("sbcl")
        .args([
            "--noinform",
            "--non-interactive",
            "--eval",
            "(require :asdf)",
            "--eval",
            "(asdf:load-system :yason)",
        ])
        .output();
    if !yason.is_ok_and(|output| output.status.success()) {
        return;
    }
    assert_eq!(
        run_command_language_contract("commonlisp", COMMON_LISP_RETURN_SOURCE),
        json!({ "answer": 42 })
    );
}

fn run_command_language_contract(language: &str, source: &str) -> runinator_models::value::Value {
    let dir = std::env::temp_dir().join(format!("runinator-{language}-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let adapter = adapter_for(language).unwrap();
    let context_path = dir.join("context.json");
    let output_path = dir.join("output.json");
    fs::write(dir.join(adapter.source_filename()), source).unwrap();
    fs::write(
        dir.join(adapter.runner_filename()),
        adapter
            .runner_source()
            .replace("/work", &dir.to_string_lossy()),
    )
    .unwrap();
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
#[ignore = "requires a running Docker daemon and the swift:6.3 image"]
fn swift_foreign_compute_compiles_then_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "swift",
        "source": r#"import Foundation

func main(_ context: Any) throws -> Any {
    let input = (context as! [String: Any])["input"] as! [String: Any]
    return ["answer": (input["value"] as! NSNumber).intValue + 1]
}
"#,
        "runtime": { "image": "swift:6.3", "setup_script": "" },
        "context": { "input": { "value": 41 } }
    });
    let mut request = request_for("code", parameters);
    request.timeout_secs = 180;

    let result = provider
        .execute_service(request, None, CancellationToken::new())
        .expect("Swift foreign compute");

    assert_eq!(result.output_json, Some(json!({ "answer": 42 })));
}

#[test]
#[ignore = "requires a running Docker daemon and the Common Lisp runtime image"]
fn common_lisp_foreign_compute_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "commonlisp",
        "source": COMMON_LISP_RETURN_SOURCE,
        "runtime": {
            "image": "clfoundation/sbcl:2.6.1-bookworm",
            "setup_script": "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends cl-alexandria cl-trivial-gray-streams cl-yason"
        },
        "context": { "input": { "value": 41 } }
    });
    let mut request = request_for("code", parameters);
    request.timeout_secs = 180;

    let result = provider
        .execute_service(request, None, CancellationToken::new())
        .expect("Common Lisp foreign compute");

    assert_eq!(result.output_json, Some(json!({ "answer": 42 })));
}

#[test]
#[ignore = "requires a running Docker daemon and the GnuCOBOL runtime image"]
fn cobol_foreign_compute_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "cobol",
        "source": COBOL_RETURN_SOURCE,
        "runtime": {
            "image": "debian:bookworm-slim",
            "setup_script": "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gnucobol"
        },
        "context": { "input": { "value": 41 } }
    });
    let mut request = request_for("code", parameters);
    request.timeout_secs = 180;

    let result = provider
        .execute_service(request, None, CancellationToken::new())
        .expect("COBOL foreign compute");

    assert_eq!(result.output_json, Some(json!({ "answer": 42 })));
}

#[test]
#[ignore = "requires a running Docker daemon and Debian GCC frontend packages"]
fn gcc_frontend_foreign_compute_returns_output() {
    let cases = [
        ("c", C_RETURN_SOURCE, "gcc libc6-dev"),
        ("cpp", CPP_RETURN_SOURCE, "g++"),
        ("fortran", FORTRAN_RETURN_SOURCE, "gfortran"),
        ("ada", ADA_RETURN_SOURCE, "gnat"),
    ];

    for (language, source, packages) in cases {
        let provider = StdProvider;
        let setup_script = format!(
            "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends {packages}"
        );
        let parameters = json!({
            "language": language,
            "source": source,
            "runtime": {
                "image": "debian:bookworm-slim",
                "setup_script": setup_script
            },
            "context": { "input": { "value": 41 } }
        });
        let mut request = request_for("code", parameters);
        request.timeout_secs = 180;

        let result = provider
            .execute_service(request, None, CancellationToken::new())
            .unwrap_or_else(|error| panic!("{language} foreign compute failed: {error}"));

        assert_eq!(
            result.output_json,
            Some(json!({ "answer": 42 })),
            "{language}"
        );
    }
}

#[test]
#[ignore = "requires a running Docker daemon and the GHC + Aeson runtime packages"]
fn haskell_foreign_compute_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "haskell",
        "source": HASKELL_IO_RETURN_SOURCE,
        "runtime": {
            "image": "debian:bookworm-slim",
            "setup_script": "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ghc libghc-aeson-dev"
        },
        "context": { "input": { "value": 41 } }
    });
    let mut request = request_for("code", parameters);
    request.timeout_secs = 240;

    let result = provider
        .execute_service(request, None, CancellationToken::new())
        .expect("Haskell foreign compute");

    assert_eq!(result.output_json, Some(json!({ "answer": 42 })));
}

#[test]
#[ignore = "requires a running Docker daemon and the OCaml + Yojson runtime packages"]
fn ocaml_foreign_compute_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "ocaml",
        "source": OCAML_RETURN_SOURCE,
        "runtime": {
            "image": "debian:bookworm-slim",
            "setup_script": "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ocaml ocaml-findlib libyojson-ocaml-dev"
        },
        "context": { "input": { "value": 41 } }
    });
    let mut request = request_for("code", parameters);
    request.timeout_secs = 240;

    let result = provider
        .execute_service(request, None, CancellationToken::new())
        .expect("OCaml foreign compute");

    assert_eq!(result.output_json, Some(json!({ "answer": 42 })));
}

#[test]
#[ignore = "requires a running Docker daemon and the Erlang + Jiffy runtime packages"]
fn erlang_escript_foreign_compute_returns_output() {
    let provider = StdProvider;
    let parameters = json!({
        "language": "erlang",
        "source": ERLANG_RETURN_SOURCE,
        "runtime": {
            "image": "debian:bookworm-slim",
            "setup_script": "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends erlang-nox erlang-jiffy"
        },
        "context": { "input": { "value": 41 } }
    });
    let mut request = request_for("code", parameters);
    request.timeout_secs = 240;

    let result = provider
        .execute_service(request, None, CancellationToken::new())
        .expect("Erlang escript foreign compute");

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
    let language = code
        .parameters
        .iter()
        .find(|parameter| parameter.name == "language")
        .expect("code language parameter");
    let RuninatorType::Enum(languages) = &language.ty else {
        panic!("code language must be a closed enum");
    };
    assert!(languages.contains(&json!("python")));
    assert!(languages.contains(&json!("vbnet")));

    let runtime = code
        .parameters
        .iter()
        .find(|parameter| parameter.name == "runtime")
        .expect("code runtime parameter");
    assert!(runtime.required);
    let RuninatorType::Struct { fields, .. } = &runtime.ty else {
        panic!("code runtime must expose structured fields");
    };
    assert!(fields.get("image").is_some_and(|field| field.required));
    assert!(fields.contains_key("environment"));
    assert!(fields.contains_key("toolchain"));
    assert!(fields.contains_key("limits"));
}
