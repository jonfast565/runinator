use std::error::Error;
use std::path::Path;

use runinator_models::errors::SendableError;
use rust_xlsxwriter::Workbook;

use super::{TableData, TableExportContext, TableExporter};

#[derive(Default)]
pub struct ExcelTableExporter;

impl ExcelTableExporter {
    pub fn new() -> Self {
        Self
    }
}

impl TableExporter for ExcelTableExporter {
    fn export(
        &self,
        path: &Path,
        table: &TableData,
        context: &TableExportContext<'_>,
    ) -> Result<(), SendableError> {
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();

        let sheet_name = context.sheet_name.unwrap_or("Sheet1");
        let sanitized_sheet = sanitize_sheet_name(sheet_name);
        worksheet.set_name(&sanitized_sheet).map_err(to_sendable)?;

        for (col_idx, header) in table.headers.iter().enumerate() {
            worksheet
                .write_string(0, col_idx as u16, header)
                .map_err(to_sendable)?;
        }

        for (row_idx, row) in table.rows.iter().enumerate() {
            for (col_idx, value) in row.iter().enumerate() {
                worksheet
                    .write_string((row_idx + 1) as u32, col_idx as u16, value)
                    .map_err(to_sendable)?;
            }
        }

        workbook.save(path).map_err(to_sendable)?;
        Ok(())
    }
}

fn sanitize_sheet_name(name: &str) -> String {
    let mut sanitized = name
        .chars()
        .map(|ch| match ch {
            ':' | '\\' | '/' | '?' | '*' | '[' | ']' => '_',
            _ if ch.is_control() => '_',
            _ => ch,
        })
        .collect::<String>();

    sanitized = sanitized.trim().trim_matches('\'').to_string();

    if sanitized.is_empty() {
        sanitized = "Sheet1".to_string();
    }

    if sanitized.len() > 31 {
        sanitized.truncate(31);
    }

    sanitized
}

fn to_sendable<E>(err: E) -> SendableError
where
    E: Error + Send + Sync + 'static,
{
    Box::new(err)
}
