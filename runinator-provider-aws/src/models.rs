
#[derive(Clone)]
pub struct AwsLogProviderOptions {
    profile: String,
    region: String,
    log_streams: Vec<String>,
    datetime_statement: String
}

