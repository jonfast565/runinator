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

mod foreign_language_adapter;
pub(crate) use foreign_language_adapter::ForeignLanguageAdapter;

mod toolchain_config;
pub(crate) use toolchain_config::ToolchainConfig;
