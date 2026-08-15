//! row mappers for packaged functions.

use super::*;

macro_rules! function_artifact_from_row {
    ($row:expr) => {{
        FunctionArtifact {
            digest: $row.get("digest"),
            size_bytes: $row.get("size_bytes"),
            uri: $row.get("uri"),
            media_type: $row.get("media_type"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_function_artifact(row) -> FunctionArtifact { function_artifact_from_row!(row) });

macro_rules! function_package_from_row {
    ($row:expr) => {{
        FunctionPackage {
            id: $row.get("id"),
            org_id: $row.get("org_id"),
            namespace: $row.get("namespace"),
            name: $row.get("name"),
            description: $row.get("description"),
            latest_version: $row.get("latest_version"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_function_package(row) -> FunctionPackage { function_package_from_row!(row) });

macro_rules! function_version_from_row {
    ($row:expr) => {{
        FunctionVersion {
            id: $row.get("id"),
            package_id: $row.get("package_id"),
            version: $row.get("version"),
            artifact_digest: $row.get("artifact_digest"),
            manifest: parse_json($row.get::<String, _>("manifest")),
            // a runtime that fails to parse falls back to a named-but-unresolved one rather than
            // failing the read: the row is still useful for listing, and execution will report the
            // real problem with the context to explain it.
            runtime: serde_json::from_str($row.get::<String, _>("runtime").as_str())
                .unwrap_or_else(|_| FunctionRuntimeSpec::new("unknown")),
            published_by: $row.get("published_by"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_function_version(row) -> FunctionVersion { function_version_from_row!(row) });

macro_rules! function_export_from_row {
    ($row:expr) => {{
        FunctionExport {
            id: $row.get("id"),
            version_id: $row.get("version_id"),
            name: $row.get("name"),
            handler: $row.get("handler"),
            description: $row.get("description"),
            input: serde_json::from_str($row.get::<String, _>("input").as_str())
                .unwrap_or_default(),
            output: serde_json::from_str($row.get::<String, _>("output").as_str())
                .unwrap_or_default(),
            // an unparseable limits blob becomes the defaults, which are bounded — never unlimited.
            limits: serde_json::from_str($row.get::<String, _>("limits").as_str())
                .unwrap_or_default(),
        }
    }};
}

row_mapper!(row_to_function_export(row) -> FunctionExport { function_export_from_row!(row) });

macro_rules! function_alias_from_row {
    ($row:expr) => {{
        FunctionAlias {
            id: $row.get("id"),
            package_id: $row.get("package_id"),
            name: $row.get("name"),
            version_id: $row.get("version_id"),
            version: $row.get("version"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_function_alias(row) -> FunctionAlias { function_alias_from_row!(row) });

macro_rules! function_adapter_workflow_from_row {
    ($row:expr) => {{
        FunctionAdapterWorkflow {
            id: $row.get("id"),
            export_id: $row.get("export_id"),
            workflow_id: $row.get("workflow_id"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_function_adapter_workflow(row) -> FunctionAdapterWorkflow {
    function_adapter_workflow_from_row!(row)
});

/// the flattened catalog view, joined across package, version, and export.
///
/// aliases are not in the join — a version may carry several — so they are filled in by the caller
/// from one extra query rather than by multiplying this result set.
macro_rules! function_catalog_entry_from_row {
    ($row:expr) => {{
        FunctionCatalogEntry {
            package_id: $row.get("package_id"),
            package_name: $row.get("package_name"),
            namespace: $row.get("namespace"),
            version_id: $row.get("version_id"),
            version: $row.get("version"),
            export_id: $row.get("export_id"),
            export_name: $row.get("export_name"),
            artifact_digest: $row.get("artifact_digest"),
            description: $row.get("description"),
            input: serde_json::from_str($row.get::<String, _>("input").as_str())
                .unwrap_or_default(),
            output: serde_json::from_str($row.get::<String, _>("output").as_str())
                .unwrap_or_default(),
            aliases: Vec::new(),
        }
    }};
}

row_mapper!(row_to_function_catalog_entry(row) -> FunctionCatalogEntry {
    function_catalog_entry_from_row!(row)
});
