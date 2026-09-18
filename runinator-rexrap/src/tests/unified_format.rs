//! Unified containers format workflow declarations without rewriting other block front ends.

use super::*;

#[test]
fn formats_workflow_and_preserves_package_and_tests() {
    let source = r#"language rexrap-1

package {
  { "name": "Example", "version": 1 }
}

workflow "Example" v1 { params { name:string } key example do { console.run(command:"echo ${params.name}") } }

tests {
  { "tests": [{ "name": "smoke" }] }
}
"#;
    let formatted = format_rrx_str(source).expect("formatted unified source");
    assert!(formatted.contains("package {\n  { \"name\": \"Example\", \"version\": 1 }\n}"));
    assert!(formatted.contains("tests {\n  { \"tests\": [{ \"name\": \"smoke\" }] }\n}"));
    assert!(formatted.contains("params {\n        name: string\n    }"));
    assert_eq!(format_rrx_str(&formatted).unwrap(), formatted);
}

#[test]
fn preserves_namespaces_and_emits_one_language_header_for_compilation() {
    let source = r#"language rexrap-1

package { { "name": "Example", "version": 1 } }

namespace example {
workflow "First" v1 { key first do { compute { return "first" } } }
}

namespace example {
workflow "Second" v1 { key second do { compute { return "second" } } }
}
"#;
    let formatted = format_rrx_str(source).expect("formatted unified source");
    assert_eq!(formatted, source);

    let workflow_source = parse_rrx_blocks(&formatted)
        .expect("parsed unified source")
        .workflows;
    assert_eq!(workflow_source.matches("language rexrap-1").count(), 1);
    assert_eq!(
        compile_all_str(&workflow_source, &CompileOptions::default())
            .unwrap()
            .len(),
        2
    );
}
