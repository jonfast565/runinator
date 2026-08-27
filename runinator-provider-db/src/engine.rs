use serde::{Deserialize, Serialize};

/// the database engines this provider can talk to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Sqlite,
    Postgres,
    Mysql,
    Mongodb,
}

impl Engine {
    pub fn as_str(&self) -> &'static str {
        match self {
            Engine::Sqlite => "sqlite",
            Engine::Postgres => "postgres",
            Engine::Mysql => "mysql",
            Engine::Mongodb => "mongodb",
        }
    }

    /// document engines take a `collection` + command instead of sql text.
    pub fn is_document_store(&self) -> bool {
        matches!(self, Engine::Mongodb)
    }

    /// the placeholder style used when rendering positional parameters in errors and docs.
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub fn placeholder(&self, index: usize) -> String {
        match self {
            Engine::Postgres => format!("${}", index + 1),
            Engine::Sqlite | Engine::Mysql => "?".to_string(),
            Engine::Mongodb => String::new(),
        }
    }
}
