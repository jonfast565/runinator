use super::*;

macro_rules! setting_from_row {
    ($row:expr) => {{
        SettingRecord {
            id: $row.get("id"),
            org_id: $row.get("org_id"),
            kind: SettingKind::from_str_lossy(&$row.get::<String, _>("kind")),
            scope: $row.get("scope"),
            name: $row.get("name"),
            value: $row.get("value"),
            updated_at: $row.get("updated_at"),
        }
    }};
}

row_mapper!(row_to_setting(row) -> SettingRecord { setting_from_row!(row) });
