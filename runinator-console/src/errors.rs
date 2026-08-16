//! the `CONSOLE` error dictionary.

use runinator_models::errors::{EngineErrors, ErrorDescriptor};

pub type Result<T> = std::result::Result<T, ConsoleError>;

/// what went wrong preparing a cell.
#[derive(Debug)]
pub enum ConsoleError {
    /// the cell did not parse as any fragment or program.
    Unparseable(String),
    /// the cell parsed but could not be compiled into something runnable.
    Uncompilable(String),
    /// the cell is empty or only comments.
    Empty,
}

impl std::fmt::Display for ConsoleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparseable(detail) => {
                write!(formatter, "CONSOLE001 - cell does not parse: {detail}")
            }
            Self::Uncompilable(detail) => {
                write!(formatter, "CONSOLE002 - cell cannot be compiled: {detail}")
            }
            Self::Empty => write!(formatter, "CONSOLE003 - cell is empty: nothing to run"),
        }
    }
}

impl std::error::Error for ConsoleError {}

pub const UNPARSEABLE: ErrorDescriptor =
    ErrorDescriptor::new("CONSOLE001", "console.unparseable", "Cell does not parse");
pub const UNCOMPILABLE: ErrorDescriptor = ErrorDescriptor::new(
    "CONSOLE002",
    "console.uncompilable",
    "Cell cannot be compiled",
);
pub const EMPTY: ErrorDescriptor =
    ErrorDescriptor::new("CONSOLE003", "console.empty", "Cell is empty");

pub const DICTIONARY: &[ErrorDescriptor] = &[UNPARSEABLE, UNCOMPILABLE, EMPTY];

/// console classifier error dictionary.
///
/// exposed so a binary can reference the codes by path; nothing constructs it.
#[allow(dead_code)]
pub struct ConsoleErrorCatalog;

impl EngineErrors for ConsoleErrorCatalog {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
