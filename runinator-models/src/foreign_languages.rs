//! Canonical foreign-language identifiers shared by authoring and execution.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForeignLanguage {
    Python,
    JavaScript,
    Bash,
    CommonLisp,
    Cobol,
    C,
    Cpp,
    Fortran,
    Ada,
    Haskell,
    Ocaml,
    Erlang,
    Ruby,
    Perl,
    Php,
    Go,
    Swift,
    PowerShell,
    CSharp,
    FSharp,
    VbNet,
}

impl ForeignLanguage {
    pub const ALL: [Self; 21] = [
        Self::Python,
        Self::JavaScript,
        Self::Bash,
        Self::CommonLisp,
        Self::Cobol,
        Self::C,
        Self::Cpp,
        Self::Fortran,
        Self::Ada,
        Self::Haskell,
        Self::Ocaml,
        Self::Erlang,
        Self::Ruby,
        Self::Perl,
        Self::Php,
        Self::Go,
        Self::Swift,
        Self::PowerShell,
        Self::CSharp,
        Self::FSharp,
        Self::VbNet,
    ];

    pub const fn canonical(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::Bash => "bash",
            Self::CommonLisp => "commonlisp",
            Self::Cobol => "cobol",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Fortran => "fortran",
            Self::Ada => "ada",
            Self::Haskell => "haskell",
            Self::Ocaml => "ocaml",
            Self::Erlang => "erlang",
            Self::Ruby => "ruby",
            Self::Perl => "perl",
            Self::Php => "php",
            Self::Go => "go",
            Self::Swift => "swift",
            Self::PowerShell => "powershell",
            Self::CSharp => "csharp",
            Self::FSharp => "fsharp",
            Self::VbNet => "vbnet",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "python" | "py" => Some(Self::Python),
            "javascript" | "js" | "node" => Some(Self::JavaScript),
            "bash" | "sh" => Some(Self::Bash),
            "commonlisp" | "common-lisp" | "common_lisp" | "lisp" | "cl" | "sbcl" => {
                Some(Self::CommonLisp)
            }
            "cobol" | "cob" | "gnucobol" => Some(Self::Cobol),
            "c" | "gcc" | "c17" => Some(Self::C),
            "cpp" | "c++" | "cxx" | "cplusplus" | "g++" => Some(Self::Cpp),
            "fortran" | "f90" | "f95" | "gfortran" => Some(Self::Fortran),
            "ada" | "adb" | "gnat" => Some(Self::Ada),
            "haskell" | "hs" | "ghc" => Some(Self::Haskell),
            "ocaml" | "ml" | "ocamlopt" => Some(Self::Ocaml),
            "erlang" | "erl" | "escript" => Some(Self::Erlang),
            "ruby" | "rb" => Some(Self::Ruby),
            "perl" | "pl" => Some(Self::Perl),
            "php" => Some(Self::Php),
            "go" | "golang" => Some(Self::Go),
            "swift" => Some(Self::Swift),
            "powershell" | "pwsh" | "ps1" => Some(Self::PowerShell),
            "csharp" | "c#" | "cs" => Some(Self::CSharp),
            "fsharp" | "f#" | "fs" => Some(Self::FSharp),
            "vbnet" | "vb.net" | "visualbasic" | "vb" => Some(Self::VbNet),
            _ => None,
        }
    }

    pub fn supported_names() -> String {
        Self::ALL
            .iter()
            .map(|language| language.canonical())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::ForeignLanguage;

    #[test]
    fn every_canonical_name_round_trips() {
        for language in ForeignLanguage::ALL {
            assert_eq!(ForeignLanguage::parse(language.canonical()), Some(language));
        }
    }
}
