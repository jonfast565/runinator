use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize)]
pub struct CommandLine {
    pub command: String,
    pub args: String
}