use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DatabaseKind {
    Postgres,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DumpFormat {
    Excel,
    Csv,
}

impl Default for DumpFormat {
    fn default() -> Self {
        DumpFormat::Excel
    }
}

impl DumpFormat {
    pub(crate) fn file_extension(&self) -> &'static str {
        match self {
            DumpFormat::Excel => "xlsx",
            DumpFormat::Csv => "csv",
        }
    }

    pub(crate) fn requires_sheet_name(&self) -> bool {
        matches!(self, DumpFormat::Excel)
    }

    pub(crate) fn mime_type(&self) -> &'static str {
        match self {
            DumpFormat::Excel => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            DumpFormat::Csv => "text/csv",
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            DumpFormat::Excel => "excel",
            DumpFormat::Csv => "csv",
        }
    }
}
