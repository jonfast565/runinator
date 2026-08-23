use std::path::Path;

use runinator_models::errors::SendableError;

pub mod csv;
pub mod excel;

#[derive(Debug, Clone)]
pub struct TableData {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl TableData {
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { headers, rows }
    }
}

#[derive(Debug, Default)]
pub struct TableExportContext<'a> {
    pub sheet_name: Option<&'a str>,
}

pub trait TableExporter: Send + Sync {
    fn export(
        &self,
        path: &Path,
        table: &TableData,
        context: &TableExportContext<'_>,
    ) -> Result<(), SendableError>;
}
