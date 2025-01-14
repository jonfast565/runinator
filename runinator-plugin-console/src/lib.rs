use log::info;
use runinator_plugin::plugin::PluginInterface;

struct ConsolePlugin;

impl ConsolePlugin {
    pub fn new() -> Self {
        ConsolePlugin {}
    }
}

#[no_mangle]
pub extern "Rust" fn new_service() -> Box<dyn PluginInterface> {
    Box::new(ConsolePlugin::new())
}

impl PluginInterface for ConsolePlugin {
    fn name(&self) -> String {
        "Console".to_string()
    }

    fn call_service(&self, name: String, args: String) {
        info!("{} -> {}", name, args);
    }
}
