#[allow(unused_imports)]
use super::*;

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
