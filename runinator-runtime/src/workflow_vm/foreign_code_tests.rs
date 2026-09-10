//! foreign language runtime defaults.

use super::*;

#[test]
fn gcc_frontend_aliases_have_explicit_runtime_defaults() {
    let cases = [
        ("gcc", "c", "gcc"),
        ("g++", "cpp", "g++"),
        ("gfortran", "fortran", "gfortran"),
        ("gnat", "ada", "gnat"),
    ];

    for (alias, canonical, package) in cases {
        let runtime = foreign_language_runtime(alias).expect(alias);
        assert_eq!(runtime.canonical, canonical, "{alias}");
        assert_eq!(runtime.image, "debian:bookworm-slim", "{alias}");
        assert!(runtime.setup_script.contains(package), "{alias}");
    }
}

#[test]
fn haskell_aliases_have_explicit_runtime_defaults() {
    for alias in ["haskell", "hs", "ghc"] {
        let runtime = foreign_language_runtime(alias).expect(alias);
        assert_eq!(runtime.canonical, "haskell", "{alias}");
        assert_eq!(runtime.image, "debian:bookworm-slim", "{alias}");
        assert!(runtime.setup_script.contains("libghc-aeson-dev"), "{alias}");
    }
}

#[test]
fn ocaml_and_erlang_aliases_have_explicit_runtime_defaults() {
    let cases = [
        ("ocaml", "ocaml", "ocaml-findlib"),
        ("ml", "ocaml", "libyojson-ocaml-dev"),
        ("ocamlopt", "ocaml", "ocaml"),
        ("erlang", "erlang", "erlang-nox"),
        ("erl", "erlang", "erlang-jiffy"),
        ("escript", "erlang", "erlang-nox"),
    ];

    for (alias, canonical, package) in cases {
        let runtime = foreign_language_runtime(alias).expect(alias);
        assert_eq!(runtime.canonical, canonical, "{alias}");
        assert_eq!(runtime.image, "debian:bookworm-slim", "{alias}");
        assert!(runtime.setup_script.contains(package), "{alias}");
    }
}
