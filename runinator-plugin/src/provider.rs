pub trait Provider : Send + Sync {
    fn name(&self) -> String;
    fn call_service(&self, call: String, args: String) -> Result<i32, Box<dyn std::error::Error>>;
}