use log::info;
use runinator_models::errors::SendableError;
use runinator_plugin::provider::Provider;

#[derive(Clone)]
pub struct SqlProvider;

impl Provider for SqlProvider {
    fn name(&self) -> String {
        "SQL".to_string()
    }

    fn call_service(&self, call: String, args: String) -> Result<i32, SendableError> {
        info!("Running call '{}' w/ args `{}`", call, args);
        Ok(0)
    }
}