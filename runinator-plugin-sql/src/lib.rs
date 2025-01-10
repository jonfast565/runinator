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
        todo!()
    }

    fn call_service(&self, _name: String, _args: Vec<u8>, _args_length: usize) {
        todo!()
    }
}
