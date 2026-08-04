use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use runinator_models::errors::SendableError;
use runinator_models::runs::NewRunArtifact;
use runinator_utilities::data_export::{
    TableExportContext, TableExporter, csv::CsvTableExporter, excel::ExcelTableExporter,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::helpers::{file_size, next_available_stem, sanitize_file_stem, to_sendable};
use crate::rowset::RowSet;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Excel,
    Csv,
}

impl ExportFormat {
    pub fn file_extension(&self) -> &'static str {
        match self {
            ExportFormat::Excel => "xlsx",
            ExportFormat::Csv => "csv",
        }
    }

    pub fn requires_sheet_name(&self) -> bool {
        matches!(self, ExportFormat::Excel)
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            ExportFormat::Excel => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            ExportFormat::Csv => "text/csv",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ExportFormat::Excel => "excel",
            ExportFormat::Csv => "csv",
        }
    }

    fn exporter(&self) -> Box<dyn TableExporter> {
        match self {
            ExportFormat::Excel => Box::new(ExcelTableExporter::new()),
            ExportFormat::Csv => Box::new(CsvTableExporter::new()),
        }
    }
}

/// optional file output for a row-returning statement. absent means results are returned
/// in-band only.
#[derive(Clone, Debug, Deserialize)]
pub struct ExportSpec {
    pub folder: String,
    #[serde(default)]
    pub format: ExportFormat,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub file_prefix: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ExportedFile {
    pub name: String,
    pub rows: usize,
    pub path: PathBuf,
    pub mime_type: String,
    pub size_bytes: i64,
    pub format: ExportFormat,
}

impl ExportedFile {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "rows": self.rows,
            "path": self.path,
            "format": self.format.as_str(),
            "size_bytes": self.size_bytes,
        })
    }

    pub fn to_artifact(&self) -> NewRunArtifact {
        NewRunArtifact {
            name: self
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.name.clone()),
            mime_type: self.mime_type.clone(),
            size_bytes: self.size_bytes,
            uri: self.path.to_string_lossy().into_owned(),
            metadata: json!({
                "provider": "db",
                "statement_name": self.name,
                "rows": self.rows,
                "format": self.format.as_str(),
            })
            .into(),
        }
    }
}

/// write a row set to disk using the shared table exporters. `counts` carries dedupe state so
/// a script exporting several same-named steps does not overwrite its own files.
pub fn export_rows(
    rows: &RowSet,
    spec: &ExportSpec,
    fallback_name: &str,
    index: usize,
    counts: &mut HashMap<String, usize>,
) -> Result<ExportedFile, SendableError> {
    let folder = PathBuf::from(&spec.folder);
    fs::create_dir_all(&folder).map_err(to_sendable)?;

    let display_name = spec
        .name
        .clone()
        .unwrap_or_else(|| fallback_name.to_string());

    let stem = sanitize_file_stem(&display_name);
    let stem = if stem.is_empty() {
        format!("statement_{:02}", index + 1)
    } else {
        stem
    };
    let prefix = spec.file_prefix.as_deref().unwrap_or("");
    let unique_stem = next_available_stem(format!("{prefix}{stem}"), counts);
    let path = folder.join(format!("{unique_stem}.{}", spec.format.file_extension()));

    let table = rows.to_table_data();
    let sheet_name = spec
        .format
        .requires_sheet_name()
        .then_some(display_name.as_str());
    let context = TableExportContext { sheet_name };

    spec.format.exporter().export(&path, &table, &context)?;
    let size_bytes = file_size(&path)?;

    Ok(ExportedFile {
        name: display_name,
        rows: rows.row_count(),
        path,
        mime_type: spec.format.mime_type().to_string(),
        size_bytes,
        format: spec.format,
    })
}
