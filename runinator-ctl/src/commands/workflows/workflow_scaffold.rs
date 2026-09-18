use super::*;

pub fn workflows_scaffold(
    path: &Path,
    requested_name: Option<&str>,
    namespace: &str,
    json_output: bool,
) -> Result<()> {
    let directory_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| err("scaffold path must end with a directory name"))?;
    let key = identifier(directory_name);
    if key.is_empty() {
        return Err(err("scaffold path must contain letters or numbers"));
    }
    let namespace = dotted_identifier(namespace)?;
    let name = requested_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| title(directory_name));
    if path.exists() {
        let mut entries = fs::read_dir(path)
            .map_err(|error| err(format!("inspect {}: {error}", path.display())))?;
        if entries.next().is_some() {
            return Err(err(format!(
                "refusing to scaffold into non-empty directory {}",
                path.display()
            )));
        }
    } else {
        fs::create_dir_all(path)
            .map_err(|error| err(format!("create {}: {error}", path.display())))?;
    }
    let source_path = path.join(format!("{key}.rrx"));
    let readme_path = path.join("README.md");
    let source = source(&name, &key, &namespace);
    fs::write(&source_path, source)
        .map_err(|error| err(format!("write {}: {error}", source_path.display())))?;
    fs::write(&readme_path, readme(&name, &source_path))
        .map_err(|error| err(format!("write {}: {error}", readme_path.display())))?;
    if json_output {
        return output::json(&json!({
            "directory": path,
            "source": source_path,
            "readme": readme_path,
            "workflow": format!("{namespace}.{key}"),
        }));
    }
    println!("created REXRAP pack at {}", path.display());
    println!("  source: {}", source_path.display());
    println!(
        "  check:  runinatorctl rexrap check {}",
        source_path.display()
    );
    println!("  test:   runinatorctl workflows test {}", path.display());
    Ok(())
}

fn identifier(value: &str) -> String {
    let mut identifier = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if identifier
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
    {
        identifier.insert(0, '_');
    }
    identifier
}

fn dotted_identifier(value: &str) -> Result<String> {
    let namespace = value.split('.').map(identifier).collect::<Vec<_>>();
    if namespace.is_empty() || namespace.iter().any(String::is_empty) {
        return Err(err("--namespace must be a dotted identifier"));
    }
    Ok(namespace.join("."))
}

fn title(value: &str) -> String {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), characters.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn source(name: &str, key: &str, namespace: &str) -> String {
    let name = serde_json::to_string(name).expect("workflow name must serialize");
    format!(
        r#"language rexrap-1

package {{
  {{ "name": {name}, "version": 1 }}
}}

namespace {namespace} {{

workflow {name} v1 {{
    params {{
        message: string = "hello from Runinator"
    }}
    key {key}

    do {{
        let greeting = console.run(command: "echo ${{params.message}}")
            routes {{
                on next {{
                    continue end
                }}
            }}
    }}
}}
}}

tests {{
  {{
    "fixtures": {{
      "default": {{
        "input": {{ "message": "hello from the test" }},
        "mocks": {{ "greeting": {{ "output": {{ "stdout": "hello from the test" }} }} }},
        "expect": {{ "status": "succeeded", "reached": ["greeting"] }}
      }}
    }},
    "tests": [
      {{
        "fixture": "default",
        "name": "greeting runs and completes"
      }}
    ]
  }}
}}
"#
    )
}

fn readme(name: &str, source_path: &Path) -> String {
    let source = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workflow.rrx");
    format!(
        "# {name}\n\nCheck and test this pack locally before applying it:\n\n```bash\nruninatorctl rexrap check {source}\nruninatorctl workflows test .\nruninatorctl workflows apply .\n```\n"
    )
}
