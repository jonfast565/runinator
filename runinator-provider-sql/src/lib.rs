use log::info;
use runinator_plugin::provider::Provider;

#[derive(Clone)]
pub struct SqlProvider;

impl Provider for SqlProvider {
    fn name(&self) -> String {
        "AWS".to_string()
    }

    fn call_service(&self, call: String, args: String) -> Result<i32, Box<dyn std::error::Error>> {
        info!("Running call '{}' w/ args `{}`", call, args);
        Ok(0)
    }
}