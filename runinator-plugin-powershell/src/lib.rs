use runinator_plugin::plugin::PluginInterface;

struct PowershellPlugin;

impl PowershellPlugin {
    pub fn new() -> Self {
        PowershellPlugin {}
    }
}

#[no_mangle]
pub extern "Rust" fn new_service() -> Box<dyn PluginInterface> {
    Box::new(PowershellPlugin::new())
}

impl PluginInterface for PowershellPlugin {
    fn name(&self) -> String {
        todo!()
    }

    fn call_service(&self, _name: String, _args: Vec<u8>, _args_length: usize) {
        todo!()
    }
}
