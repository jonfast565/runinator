use super::ForeignLanguageAdapter;

pub(super) static OCAML: Ocaml = Ocaml;

pub(super) struct Ocaml;

impl ForeignLanguageAdapter for Ocaml {
    fn canonical(&self) -> &'static str {
        "ocaml"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.ml"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.sh"
    }

    fn runner_source(&self) -> &'static str {
        r#"#!/usr/bin/env bash
set -euo pipefail

build_dir=/tmp/runinator-ocaml
mkdir -p "${build_dir}"
cp /work/foreign.ml /work/runinator_main.ml "${build_dir}/"
cd "${build_dir}"
ocamlfind ocamlopt -package yojson -linkpkg \
    foreign.ml \
    runinator_main.ml \
    -o /tmp/runinator_foreign
/tmp/runinator_foreign "$@"
"#
    }

    fn additional_files(&self) -> &'static [(&'static str, &'static str)] {
        &[("runinator_main.ml", OCAML_MAIN)]
    }

    fn execute(&self) -> &'static str {
        "bash /work/runinator_runner.sh"
    }
}

const OCAML_MAIN: &str = r#"let () =
  let context_path = Sys.getenv "RUNINATOR_CONTEXT" in
  let output_path = Sys.getenv "RUNINATOR_OUTPUT" in
  let context = Yojson.Safe.from_file context_path in
  let result = Foreign.runinator_main context in
  Yojson.Safe.to_file output_path result
"#;
