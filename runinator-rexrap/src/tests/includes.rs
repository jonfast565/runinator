//! `include file` and `include dir`: what they read, the source-dir escape they refuse,
//! and the shape a directory listing lowers to.

use super::*;

#[test]
fn lists_included_file_paths() {
    let src = r#"
        workflow "Includes" v1 {
            alias shared = { script: file("scripts/shared.py") }
            node go <- console.run(command: file("scripts/job.py"), ...shared)
        }
    "#;
    let mut paths =
        crate::included_file_paths(src, std::path::Path::new("/pack")).expect("include paths");
    paths.sort();
    assert_eq!(
        paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec!["/pack/scripts/job.py", "/pack/scripts/shared.py"]
    );
}
#[test]
fn lowers_file_include_relative_to_source_dir() {
    let mut dir = std::env::temp_dir();
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    dir.push(format!("runinator-rexrap-include-{unique}"));
    fs::create_dir_all(dir.join("scripts")).expect("mkdir");
    fs::write(dir.join("scripts/job.py"), "print('from file')\n").expect("write include");

    let src = r#"
        workflow "FileInclude" v1 {
            node go <- console.run(command: file("scripts/job.py"))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        providers: Vec::new(),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with include");
    assert_eq!(
        action_config_value(&definition, "command").as_str(),
        Some("print('from file')\n")
    );

    fs::remove_dir_all(dir).expect("cleanup");
}
#[test]
fn file_include_requires_source_dir() {
    let src = r#"
        workflow "FileInclude" v1 {
            node go <- console.run(command: file("scripts/job.py"))
        }
    "#;
    match compile_str(src, &CompileOptions::default()) {
        Err(RexRapError::Semantic { message, .. }) => {
            assert!(message.contains("source directory"), "{message}");
        }
        other => panic!("expected source directory error, got {other:?}"),
    }
}
#[test]
fn file_include_cannot_escape_source_dir() {
    let src = r#"
        workflow "FileInclude" v1 {
            node go <- console.run(command: file("../job.py"))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(std::env::temp_dir()),
        providers: Vec::new(),
        ..CompileOptions::default()
    };
    match compile_str(src, &options) {
        Err(RexRapError::Semantic { message, .. }) => {
            assert!(message.contains("relative"), "{message}");
        }
        other => panic!("expected unsafe path error, got {other:?}"),
    }
}
fn dir_fixture(label: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    dir.push(format!("runinator-rexrap-{label}-{unique}"));
    fs::create_dir_all(dir.join("scripts/lib")).expect("mkdir");
    fs::write(dir.join("scripts/job.py"), "a").expect("write");
    fs::write(dir.join("scripts/setup.py"), "b").expect("write");
    fs::write(dir.join("scripts/lib/util.py"), "c").expect("write");
    dir
}
fn dir_listing(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().expect("string entry").to_string())
            .collect(),
        other => panic!("expected array listing, got {other:?}"),
    }
}
#[test]
fn dir_include_lists_top_level_by_default() {
    let dir = dir_fixture("dir-top");
    let src = r#"
        workflow "DirInclude" v1 {
            node go <- console.run(command: dir("scripts"))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        providers: Vec::new(),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with dir");
    assert_eq!(
        dir_listing(action_config_value(&definition, "command")),
        vec!["job.py".to_string(), "setup.py".to_string()]
    );
    fs::remove_dir_all(dir).expect("cleanup");
}
#[test]
fn dir_include_recurses_with_relative_paths() {
    let dir = dir_fixture("dir-recursive");
    let src = r#"
        workflow "DirInclude" v1 {
            node go <- console.run(command: dir("scripts", true))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        providers: Vec::new(),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with recursive dir");
    assert_eq!(
        dir_listing(action_config_value(&definition, "command")),
        vec![
            "job.py".to_string(),
            "lib/util.py".to_string(),
            "setup.py".to_string(),
        ]
    );
    fs::remove_dir_all(dir).expect("cleanup");
}
#[test]
fn dir_include_depth_cap_stops_descent() {
    let dir = dir_fixture("dir-depth");
    let src = r#"
        workflow "DirInclude" v1 {
            node go <- console.run(command: dir("scripts", true, 1))
        }
    "#;
    let options = CompileOptions {
        source_dir: Some(dir.clone()),
        providers: Vec::new(),
        ..CompileOptions::default()
    };
    let definition = compile_str(src, &options).expect("compile with depth cap");
    assert_eq!(
        dir_listing(action_config_value(&definition, "command")),
        vec!["job.py".to_string(), "setup.py".to_string()]
    );
    fs::remove_dir_all(dir).expect("cleanup");
}
#[test]
fn dir_include_requires_source_dir() {
    let src = r#"
        workflow "DirInclude" v1 {
            node go <- console.run(command: dir("scripts"))
        }
    "#;
    match compile_str(src, &CompileOptions::default()) {
        Err(RexRapError::Semantic { message, .. }) => {
            assert!(message.contains("source directory"), "{message}");
        }
        other => panic!("expected source directory error, got {other:?}"),
    }
}
#[test]
fn dir_include_round_trips_through_formatter() {
    let src = r#"workflow "DirInclude" v1 {
    node go <- console.run(command: dir("scripts", true, 2))
}
"#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("dir(\"scripts\", true, 2)"),
        "{formatted}"
    );
}
