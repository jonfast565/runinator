use super::ForeignLanguageAdapter;

pub(super) static RUBY: Ruby = Ruby;

pub(super) struct Ruby;

impl ForeignLanguageAdapter for Ruby {
    fn canonical(&self) -> &'static str {
        "ruby"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.rb"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.rb"
    }

    fn runner_source(&self) -> &'static str {
        r#"require "json"
load File.join(__dir__, "foreign.rb")

context = JSON.parse(File.read(ENV.fetch("RUNINATOR_CONTEXT")))
result = main(context)
File.write(ENV.fetch("RUNINATOR_OUTPUT"), JSON.generate(result))
"#
    }

    fn execute(&self) -> &'static str {
        "ruby /work/runinator_runner.rb"
    }
}
