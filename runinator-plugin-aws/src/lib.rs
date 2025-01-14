use runinator_plugin::plugin::PluginInterface;

struct AwsPlugin;

impl AwsPlugin {
    pub fn new() -> AwsPlugin {
        AwsPlugin {}
    }

    fn _aws_login() {
        
    }
}

#[no_mangle]
pub extern "Rust" fn new_service() -> Box<dyn PluginInterface> {
    Box::new(AwsPlugin::new())
}

impl PluginInterface for AwsPlugin {
    fn name(&self) -> String {
        "Amazon Web Services".to_string()
    }

    fn call_service(&self, _name: String, _args: Vec<u8>, _args_length: usize) {
        todo!()
    }
}
