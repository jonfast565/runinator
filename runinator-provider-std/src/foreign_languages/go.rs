use super::ForeignLanguageAdapter;

pub(super) static GO: Go = Go;

pub(super) struct Go;

impl ForeignLanguageAdapter for Go {
    fn canonical(&self) -> &'static str {
        "go"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.go"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.go"
    }

    fn runner_source(&self) -> &'static str {
        r#"package main

import (
	"encoding/json"
	"os"
)

func main() {
	contextFile, err := os.Open(os.Getenv("RUNINATOR_CONTEXT"))
	if err != nil {
		panic(err)
	}
	defer contextFile.Close()

	var context any
	if err := json.NewDecoder(contextFile).Decode(&context); err != nil {
		panic(err)
	}

	outputFile, err := os.Create(os.Getenv("RUNINATOR_OUTPUT"))
	if err != nil {
		panic(err)
	}
	defer outputFile.Close()
	if err := json.NewEncoder(outputFile).Encode(Main(context)); err != nil {
		panic(err)
	}
}
"#
    }

    fn execute(&self) -> &'static str {
        "go run /work/runinator_runner.go /work/foreign.go"
    }
}
