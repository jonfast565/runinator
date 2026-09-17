#[allow(unused_imports)]
use super::*;

pub struct RenderedSql(pub(super) String);

impl SqlSafeStr for &RenderedSql {
    fn into_sql_str(self) -> SqlStr {
        AssertSqlSafe(self.0.as_str()).into_sql_str()
    }
}
