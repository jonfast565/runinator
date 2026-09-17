#[allow(unused_imports)]
use super::*;

pub trait DatabaseConnector: Send + Sync {
    /// create the database if it is missing. returns whether it was created.
    fn ensure_database(
        &self,
        spec: &ProvisionSpec,
        timeout: Duration,
    ) -> Result<bool, SendableError>;

    fn query(&self, statement: &StatementSpec, timeout: Duration) -> Result<RowSet, SendableError>;

    fn execute(
        &self,
        statement: &StatementSpec,
        timeout: Duration,
    ) -> Result<ExecOutcome, SendableError>;

    /// run statements in order, optionally inside a single transaction.
    fn script(
        &self,
        statements: &[StatementSpec],
        transactional: bool,
        timeout: Duration,
    ) -> Result<Vec<StepOutcome>, SendableError>;

    fn seed(&self, seeds: &[SeedSpec], timeout: Duration) -> Result<u64, SendableError>;

    fn inspect(&self, timeout: Duration) -> Result<Vec<TableInfo>, SendableError>;
}
