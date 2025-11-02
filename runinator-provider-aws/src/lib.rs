mod dynamo;

use log::info;
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_plugin::provider::Provider;

#[derive(Clone)]
pub struct AwsProvider;

impl Provider for AwsProvider {
    fn name(&self) -> String {
        "AWS".to_string()
    }

    fn call_service(&self, call: String, args: String, timeout: i64) -> Result<i32, SendableError> {
        info!("Running call '{}' w/ args `{}`", call, args);

        match call.as_str() {
            "dynamo_dump" => {
                dynamo::run_dynamo_dump(&args, timeout)?;
                Ok(0)
            }
            _ => Err(Box::new(RuntimeError::new(
                "UNSUPPORTED_CALL".to_string(),
                format!("Unsupported AWS provider call '{call}'"),
            ))),
        }
    }
}
