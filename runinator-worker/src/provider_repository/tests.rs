use std::{collections::HashMap, path::PathBuf};

use runinator_plugin::plugin::Plugin;

#[test]
fn provider_metadata_uses_builtin_when_plugin_has_same_name() {
    let libraries = HashMap::from([(
        "Console".to_string(),
        Plugin {
            file_name: PathBuf::from("missing-console-plugin"),
            name: "Console".to_string(),
        },
    )]);

    let providers = super::provider_metadata(&libraries);
    let console_count = providers
        .iter()
        .filter(|provider| provider.name == "Console")
        .count();

    assert_eq!(console_count, 1);
}
