use super::*;

#[test]
fn skips_invalid_plugin_file_during_discovery() {
    let dir = std::env::temp_dir().join(format!("runinator-plugin-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let plugin_path = dir.join(format!("bad.{}", utilities::get_library_extension()));
    fs::write(&plugin_path, b"not a dynamic library").unwrap();

    let libraries = load_libraries_from_path(dir.to_str().unwrap()).unwrap();

    assert!(libraries.is_empty());
    let _ = fs::remove_dir_all(&dir);
}
