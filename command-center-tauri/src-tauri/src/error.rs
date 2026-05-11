use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("No service discovered")]
    NoService,
    #[error("{0}")]
    Url(#[from] url::ParseError),
    #[error("{0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Unexpected(String),
}

impl Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
