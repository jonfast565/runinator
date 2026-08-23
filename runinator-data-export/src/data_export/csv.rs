use std::error::Error;
use std::path::Path;

use csv::WriterBuilder;
use runinator_models::errors::SendableError;

use super::{TableData, TableExportContext, TableExporter};

#[derive(Default)]
pub struct CsvTableExporter;

impl CsvTableExporter {
    pub fn new() -> Self {
        Self
    }
}

impl TableExporter for CsvTableExporter {
    fn export(
        &self,
        path: &Path,
        table: &TableData,
        _context: &TableExportContext<'_>,
    ) -> Result<(), SendableError> {
        let mut writer = WriterBuilder::new()
            .has_headers(true)
            .from_path(path)
            .map_err(to_sendable)?;

        writer.write_record(&table.headers).map_err(to_sendable)?;

        for row in &table.rows {
            writer.write_record(row).map_err(to_sendable)?;
        }

        writer.flush().map_err(to_sendable)?;
        Ok(())
    }
}

fn to_sendable<E>(err: E) -> SendableError
where
    E: Error + Send + Sync + 'static,
{
    Box::new(err)
}
