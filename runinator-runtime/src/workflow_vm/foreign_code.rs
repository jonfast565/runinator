//! foreign code behavior for the workflow interpreter.

use runinator_models::value::Value;

const COMMON_LISP_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends cl-alexandria cl-trivial-gray-streams cl-yason
rm -rf /var/lib/apt/lists/*"#;

const COBOL_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gnucobol
rm -rf /var/lib/apt/lists/*"#;

const C_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gcc libc6-dev
rm -rf /var/lib/apt/lists/*"#;

const CPP_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends g++
rm -rf /var/lib/apt/lists/*"#;

const FORTRAN_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gfortran
rm -rf /var/lib/apt/lists/*"#;

const ADA_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gnat
rm -rf /var/lib/apt/lists/*"#;

const HASKELL_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ghc libghc-aeson-dev
rm -rf /var/lib/apt/lists/*"#;

const OCAML_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ocaml ocaml-findlib libyojson-ocaml-dev
rm -rf /var/lib/apt/lists/*"#;

const ERLANG_SETUP: &str = r#"apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends erlang-nox erlang-jiffy
rm -rf /var/lib/apt/lists/*"#;

struct ForeignLanguageRuntime {
    canonical: &'static str,
    image: &'static str,
    setup_script: &'static str,
    executable: &'static str,
}

fn foreign_language_runtime(language: &str) -> Option<ForeignLanguageRuntime> {
    use runinator_models::foreign_languages::ForeignLanguage;
    let language = ForeignLanguage::parse(language)?;
    let (image, setup_script, executable) = match language {
        ForeignLanguage::Python => ("python:3.12", "", "python"),
        ForeignLanguage::JavaScript => ("node:22", "", "node"),
        ForeignLanguage::Bash => ("bash:5.2", "", "bash"),
        ForeignLanguage::CommonLisp => (
            "clfoundation/sbcl:2.6.1-bookworm",
            COMMON_LISP_SETUP,
            "sbcl",
        ),
        ForeignLanguage::Cobol => ("debian:bookworm-slim", COBOL_SETUP, "cobc"),
        ForeignLanguage::C => ("debian:bookworm-slim", C_SETUP, "gcc"),
        ForeignLanguage::Cpp => ("debian:bookworm-slim", CPP_SETUP, "g++"),
        ForeignLanguage::Fortran => ("debian:bookworm-slim", FORTRAN_SETUP, "gfortran"),
        ForeignLanguage::Ada => ("debian:bookworm-slim", ADA_SETUP, "gnatmake"),
        ForeignLanguage::Haskell => ("debian:bookworm-slim", HASKELL_SETUP, "ghc"),
        ForeignLanguage::Ocaml => ("debian:bookworm-slim", OCAML_SETUP, "ocamlfind"),
        ForeignLanguage::Erlang => ("debian:bookworm-slim", ERLANG_SETUP, "escript"),
        ForeignLanguage::Ruby => ("ruby:3.3", "", "ruby"),
        ForeignLanguage::Perl => ("perl:5.40", "", "perl"),
        ForeignLanguage::Php => ("php:8.3-cli", "", "php"),
        ForeignLanguage::Go => ("golang:1.26", "", "go"),
        ForeignLanguage::Swift => ("swift:6.3", "", "swiftc"),
        ForeignLanguage::PowerShell => ("mcr.microsoft.com/dotnet/sdk:8.0", "", "pwsh"),
        ForeignLanguage::CSharp | ForeignLanguage::FSharp | ForeignLanguage::VbNet => {
            ("mcr.microsoft.com/dotnet/sdk:10.0", "", "dotnet")
        }
    };
    Some(ForeignLanguageRuntime {
        canonical: language.canonical(),
        image,
        setup_script,
        executable,
    })
}

fn merge_runtime_value(target: &mut Value, override_value: &Value) {
    if let (Some(target), Some(override_object)) =
        (target.as_object_mut(), override_value.as_object())
    {
        for (key, value) in override_object {
            if let Some(existing) = target.get_mut(key) {
                merge_runtime_value(existing, value);
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    } else {
        *target = override_value.clone();
    }
}

/// Complete the authoring-time `std.code` payload at the durable effect boundary. The runtime and
/// context are run inputs, not authored source: both are frozen into the effect so a retry cannot
/// observe a later settings edit or a later continuation state.
pub(super) fn enrich_foreign_code_input(
    mut input: Value,
    context: &Value,
) -> Result<Value, String> {
    let object = input
        .as_object_mut()
        .ok_or_else(|| "foreign compute configuration must be an object".to_string())?;
    let language = object
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let runtime = foreign_language_runtime(language).ok_or_else(|| {
        format!(
            "unsupported foreign language '{language}'; supported languages: {}",
            runinator_models::foreign_languages::ForeignLanguage::supported_names()
        )
    })?;
    let configured = context
        .get("config")
        .and_then(|config| config.get("foreign_languages"))
        .and_then(|languages| languages.get(runtime.canonical))
        .cloned();
    if configured.as_ref().is_some_and(|value| !value.is_object()) {
        return Err(format!(
            "config.foreign_languages.{} must be a runtime object",
            runtime.canonical
        ));
    }
    let mut runtime_object = runinator_models::json!({
        "image": runtime.image,
        "setup_script": runtime.setup_script,
        "environment": {},
        "toolchain": {
            "executable": runtime.executable,
            "build_args": [],
            "run_args": [],
        },
        "limits": {
            "memory_mb": 2048,
            "cpu_millis": 2000,
            "pids": 256,
            "tmpfs_mb": 512,
            "max_output_bytes": 1048576,
        },
    });
    if let Some(configured) = configured {
        merge_runtime_value(&mut runtime_object, &configured);
    }
    object.insert(
        "language".into(),
        Value::String(runtime.canonical.to_string()),
    );
    object.insert("context".into(), context.clone());
    object.insert("runtime".into(), runtime_object);
    Ok(input)
}

#[cfg(test)]
#[path = "foreign_code_tests.rs"]
mod foreign_code_tests;
