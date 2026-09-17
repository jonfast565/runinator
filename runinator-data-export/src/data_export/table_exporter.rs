#[allow(unused_imports)]
use super::*;

pub trait TableExporter: Send + Sync {
    fn export(
        &self,
        path: &Path,
        table: &TableData,
        context: &TableExportContext<'_>,
    ) -> Result<(), SendableError>;
}
