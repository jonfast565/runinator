mod ada;
mod bash;
mod c;
mod cobol;
mod common_lisp;
mod cpp;
mod csharp;
mod erlang;
mod fortran;
mod fsharp;
mod go;
mod haskell;
mod javascript;
mod ocaml;
mod perl;
mod php;
mod powershell;
mod python;
mod ruby;
mod swift;
mod vbnet;

use runinator_models::{errors::SendableError, foreign_languages::ForeignLanguage};

use crate::errors::INVALID_CODE;

pub(crate) trait ForeignLanguageAdapter: Sync {
    fn canonical(&self) -> &'static str;
    fn source_filename(&self) -> &'static str;
    fn runner_filename(&self) -> &'static str;
    fn runner_source(&self) -> &'static str;
    fn additional_files(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
    fn execute(&self) -> &'static str;

    fn default_executable(&self) -> &'static str {
        match self.canonical() {
            "python" => "python",
            "javascript" => "node",
            "bash" => "bash",
            "commonlisp" => "sbcl",
            "cobol" => "cobc",
            "c" => "gcc",
            "cpp" => "g++",
            "fortran" => "gfortran",
            "ada" => "gnatmake",
            "haskell" => "ghc",
            "ocaml" => "ocamlfind",
            "erlang" => "escript",
            "ruby" => "ruby",
            "perl" => "perl",
            "php" => "php",
            "go" => "go",
            "swift" => "swiftc",
            "powershell" => "pwsh",
            "csharp" | "fsharp" | "vbnet" => "dotnet",
            _ => unreachable!("registered adapter has a default executable"),
        }
    }

    fn compiler_in_runner(&self) -> bool {
        matches!(
            self.canonical(),
            "cobol" | "c" | "cpp" | "fortran" | "ada" | "haskell" | "ocaml" | "swift"
        )
    }

    fn toolchain_prefix(&self) -> &'static str {
        match self.canonical() {
            "go" => "go run",
            "csharp" | "fsharp" | "vbnet" => "dotnet run",
            "ocaml" => "ocamlfind ocamlopt",
            _ => self.default_executable(),
        }
    }

    fn rendered_toolchain_prefix(&self, toolchain: &ToolchainConfig) -> String {
        let suffix = self
            .toolchain_prefix()
            .strip_prefix(self.default_executable())
            .expect("toolchain prefix begins with its default executable");
        let mut rendered = shell_quote(&toolchain.executable);
        rendered.push_str(suffix);
        append_quoted_args(&mut rendered, &toolchain.build_args);
        rendered
    }

    fn rendered_runner_source(&self, toolchain: &ToolchainConfig) -> String {
        let source = self.runner_source();
        if !self.compiler_in_runner() {
            return source.to_string();
        }
        source.replacen(
            self.toolchain_prefix(),
            &self.rendered_toolchain_prefix(toolchain),
            1,
        )
    }

    fn rendered_execute(&self, toolchain: &ToolchainConfig) -> String {
        if self.compiler_in_runner() {
            let mut command = self.execute().to_string();
            append_quoted_args(&mut command, &toolchain.run_args);
            return command;
        }

        let execute = self.execute();
        let remainder = execute
            .strip_prefix(self.toolchain_prefix())
            .expect("adapter execute command begins with its toolchain prefix");
        let mut command = self.rendered_toolchain_prefix(toolchain);
        command.push_str(remainder);
        append_quoted_args(&mut command, &toolchain.run_args);
        command
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ToolchainConfig {
    pub(crate) executable: String,
    pub(crate) build_args: Vec<String>,
    pub(crate) run_args: Vec<String>,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn append_quoted_args(command: &mut String, args: &[String]) {
    for arg in args {
        command.push(' ');
        command.push_str(&shell_quote(arg));
    }
}

pub(crate) fn adapter_for(
    language: &str,
) -> Result<&'static dyn ForeignLanguageAdapter, SendableError> {
    let parsed = ForeignLanguage::parse(language).ok_or_else(|| {
        INVALID_CODE.error(format!(
            "unsupported foreign language '{language}'; supported languages: {}",
            ForeignLanguage::supported_names()
        ))
    })?;
    Ok(match parsed {
        ForeignLanguage::Python => &python::PYTHON,
        ForeignLanguage::JavaScript => &javascript::JAVASCRIPT,
        ForeignLanguage::Bash => &bash::BASH,
        ForeignLanguage::CommonLisp => &common_lisp::COMMON_LISP,
        ForeignLanguage::Cobol => &cobol::COBOL,
        ForeignLanguage::C => &c::C_LANGUAGE,
        ForeignLanguage::Cpp => &cpp::CPP,
        ForeignLanguage::Fortran => &fortran::FORTRAN,
        ForeignLanguage::Ada => &ada::ADA,
        ForeignLanguage::Haskell => &haskell::HASKELL,
        ForeignLanguage::Ocaml => &ocaml::OCAML,
        ForeignLanguage::Erlang => &erlang::ERLANG,
        ForeignLanguage::Ruby => &ruby::RUBY,
        ForeignLanguage::Perl => &perl::PERL,
        ForeignLanguage::Php => &php::PHP,
        ForeignLanguage::Go => &go::GO,
        ForeignLanguage::Swift => &swift::SWIFT,
        ForeignLanguage::PowerShell => &powershell::POWERSHELL,
        ForeignLanguage::CSharp => &csharp::CSHARP,
        ForeignLanguage::FSharp => &fsharp::FSHARP,
        ForeignLanguage::VbNet => &vbnet::VBNET,
    })
}
