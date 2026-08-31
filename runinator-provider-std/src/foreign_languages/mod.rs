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

use runinator_models::errors::SendableError;

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
}

pub(crate) fn adapter_for(
    language: &str,
) -> Result<&'static dyn ForeignLanguageAdapter, SendableError> {
    match language {
        "python" | "py" => Ok(&python::PYTHON),
        "javascript" | "js" | "node" => Ok(&javascript::JAVASCRIPT),
        "bash" | "sh" => Ok(&bash::BASH),
        "commonlisp" | "common-lisp" | "common_lisp" | "lisp" | "cl" | "sbcl" => {
            Ok(&common_lisp::COMMON_LISP)
        }
        "cobol" | "cob" | "gnucobol" => Ok(&cobol::COBOL),
        "c" | "gcc" | "c17" => Ok(&c::C_LANGUAGE),
        "cpp" | "c++" | "cxx" | "cplusplus" | "g++" => Ok(&cpp::CPP),
        "fortran" | "f90" | "f95" | "gfortran" => Ok(&fortran::FORTRAN),
        "ada" | "adb" | "gnat" => Ok(&ada::ADA),
        "haskell" | "hs" | "ghc" => Ok(&haskell::HASKELL),
        "ocaml" | "ml" | "ocamlopt" => Ok(&ocaml::OCAML),
        "erlang" | "erl" | "escript" => Ok(&erlang::ERLANG),
        "ruby" | "rb" => Ok(&ruby::RUBY),
        "perl" | "pl" => Ok(&perl::PERL),
        "php" => Ok(&php::PHP),
        "go" | "golang" => Ok(&go::GO),
        "swift" => Ok(&swift::SWIFT),
        "powershell" | "pwsh" | "ps1" => Ok(&powershell::POWERSHELL),
        "csharp" | "c#" | "cs" => Ok(&csharp::CSHARP),
        "fsharp" | "f#" | "fs" => Ok(&fsharp::FSHARP),
        "vbnet" | "vb.net" | "visualbasic" | "vb" => Ok(&vbnet::VBNET),
        other => Err(INVALID_CODE.error(format!(
            "unsupported foreign language '{other}'; supported languages: python, javascript, bash, commonlisp, cobol, c, cpp, fortran, ada, haskell, ocaml, erlang, ruby, perl, php, go, swift, powershell, csharp, fsharp, vbnet"
        ))),
    }
}
