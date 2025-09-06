use runinator_models::errors::SendableError;

pub trait Provider: Send + Sync {
    fn name(&self) -> String;
    fn call_service(&self, call: String, args: String, timeout_secs: i64) -> Result<i32, SendableError>;
}
