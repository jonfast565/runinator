use log::info;
use runinator_plugin::plugin::PluginInterface;

struct SqlPlugin;

impl SqlPlugin {
    pub fn new() -> Self {
        SqlPlugin {}
    }
}

#[no_mangle]
pub extern "Rust" fn new_service() -> Box<dyn PluginInterface> {
    Box::new(SqlPlugin::new())
}

impl PluginInterface for SqlPlugin {
    fn name(&self) -> String {
        "SQL".to_string()
    }

    fn call_service(&self, name: String, args: String) {
        info!("{} -> {}", name, args);
    }
}
