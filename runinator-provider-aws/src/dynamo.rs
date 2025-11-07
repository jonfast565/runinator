use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use aws_config::BehaviorVersion;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::config::Region;
use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::AttributeValue;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use log::info;
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_utilities::data_export::{
    TableData, TableExportContext, TableExporter, csv::CsvTableExporter, excel::ExcelTableExporter,
};
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use tokio::time;

pub fn run_dynamo_dump(args: &str, timeout_secs: i64) -> Result<(), SendableError> {
    let request: DynamoDumpRequest = serde_json::from_str(args).map_err(to_sendable)?;
    let timeout = normalize_timeout(timeout_secs);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("runinator-aws")
        .build()
        .map_err(to_sendable)?;

    runtime.block_on(async move { execute_dump(request, timeout).await })
}

async fn execute_dump(request: DynamoDumpRequest, timeout: Duration) -> Result<(), SendableError> {
    let timeout_secs = timeout.as_secs();
    let fut = async {
        let client = build_client(&request).await?;
        let items = match request.query_type {
            DynamoQueryType::Query => query_items(&client, &request).await?,
            DynamoQueryType::Partiql => execute_partiql(&client, &request).await?,
        };
        info!(
            "Fetched {} item(s) from DynamoDB table {}",
            items.len(),
            request.table_name
        );

        let table = items_to_table_data(&items);
        export_results(&request, table)?;
        Ok(())
    };

    match time::timeout(timeout, fut).await {
        Ok(result) => result,
        Err(_) => Err(Box::new(RuntimeError::new(
            "DYNAMO_TIMEOUT".to_string(),
            format!(
                "Timed out after {} second(s) while querying DynamoDB table {}",
                timeout_secs, request.table_name
            ),
        ))),
    }
}

async fn build_client(request: &DynamoDumpRequest) -> Result<Client, SendableError> {
    let explicit_region = request
        .region
        .as_ref()
        .map(|name| Region::new(name.to_string()));

    let region_provider = RegionProviderChain::first_try(explicit_region)
        .or_default_provider()
        .or_else("us-east-1");

    let shared_config = aws_config::defaults(BehaviorVersion::v2025_08_07())
        .region(region_provider)
        .load()
        .await;

    Ok(Client::new(&shared_config))
}

async fn query_items(
    client: &Client,
    request: &DynamoDumpRequest,
) -> Result<Vec<HashMap<String, AttributeValue>>, SendableError> {
    let key_condition = request.key_condition_expression.as_ref().ok_or_else(|| {
        invalid_request(
            "MISSING_KEY_CONDITION",
            "key_condition_expression is required when query_type is 'query'",
        )
    })?;
    let key_condition_expr = key_condition.as_str();

    let expression_values =
        convert_expression_attribute_values(&request.expression_attribute_values)?;
    let expression_names = if request.expression_attribute_names.is_empty() {
        None
    } else {
        Some(request.expression_attribute_names.clone())
    };

    let mut items = Vec::new();
    let mut exclusive_start_key: Option<HashMap<String, AttributeValue>> = None;

    loop {
        let mut builder = client.query();
        builder = builder.table_name(&request.table_name);
        builder = builder.key_condition_expression(key_condition_expr);

        if let Some(index_name) = &request.index_name {
            builder = builder.index_name(index_name);
        }
        if let Some(filter) = &request.filter_expression {
            builder = builder.filter_expression(filter);
        }
        if let Some(projection) = &request.projection_expression {
            builder = builder.projection_expression(projection);
        }
        if let Some(ref names) = expression_names {
            builder = builder.set_expression_attribute_names(Some(names.clone()));
        }
        if let Some(ref values) = expression_values {
            builder = builder.set_expression_attribute_values(Some(values.clone()));
        }
        if let Some(limit) = sanitize_limit(request.limit) {
            builder = builder.limit(limit);
        }
        if let Some(flag) = request.consistent_read {
            builder = builder.consistent_read(flag);
        }
        if let Some(flag) = request.scan_index_forward {
            builder = builder.scan_index_forward(flag);
        }
        if let Some(ref start_key) = exclusive_start_key {
            builder = builder.set_exclusive_start_key(Some(start_key.clone()));
        }

        let response = builder.send().await.map_err(to_sendable)?;

        if let Some(page_items) = response.items {
            items.extend(page_items);
        }

        if let Some(last_key) = response.last_evaluated_key {
            exclusive_start_key = Some(last_key);
        } else {
            break;
        }
    }

    Ok(items)
}

async fn execute_partiql(
    client: &Client,
    request: &DynamoDumpRequest,
) -> Result<Vec<HashMap<String, AttributeValue>>, SendableError> {
    let statement = request.partiql_statement.as_ref().ok_or_else(|| {
        invalid_request(
            "MISSING_PARTIQL_STATEMENT",
            "partiql_statement is required when query_type is 'partiql'",
        )
    })?;
    let statement_text = statement.as_str();

    let parameters = convert_partiql_parameters(request.partiql_parameters.as_ref())?;

    let mut items = Vec::new();
    let mut next_token: Option<String> = None;

    loop {
        let mut builder = client.execute_statement().statement(statement_text);
        if let Some(ref params) = parameters {
            builder = builder.set_parameters(Some(params.clone()));
        }
        if let Some(token) = &next_token {
            builder = builder.set_next_token(Some(token.clone()));
        }
        if let Some(limit) = sanitize_limit(request.limit) {
            builder = builder.limit(limit);
        }
        if let Some(flag) = request.consistent_read {
            builder = builder.consistent_read(flag);
        }

        let response = builder.send().await.map_err(to_sendable)?;

        if let Some(page_items) = response.items {
            items.extend(page_items);
        }

        if let Some(token) = response.next_token {
            next_token = Some(token);
        } else {
            break;
        }
    }

    Ok(items)
}

fn export_results(request: &DynamoDumpRequest, table: TableData) -> Result<(), SendableError> {
    let dump_dir = PathBuf::from(&request.dump_folder);
    fs::create_dir_all(&dump_dir).map_err(to_sendable)?;

    let file_stem = request
        .file_name
        .as_deref()
        .map(sanitize_file_stem)
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| sanitize_file_stem(&request.table_name));

    let output_path = dump_dir.join(format!("{}.{}", file_stem, request.format.file_extension()));

    let exporter: Box<dyn TableExporter> = match request.format {
        DumpFormat::Excel => Box::new(ExcelTableExporter::new()),
        DumpFormat::Csv => Box::new(CsvTableExporter::new()),
    };

    let mut sheet_name_holder: Option<String> = None;
    if request.format.requires_sheet_name() {
        sheet_name_holder = Some(
            request
                .sheet_name
                .clone()
                .unwrap_or_else(|| request.table_name.clone()),
        );
    }

    let context = TableExportContext {
        sheet_name: sheet_name_holder.as_deref(),
    };

    exporter.export(&output_path, &table, &context)?;
    info!(
        "Exported {} row(s) to {}",
        table.rows.len(),
        output_path.display()
    );

    Ok(())
}

fn items_to_table_data(items: &[HashMap<String, AttributeValue>]) -> TableData {
    let mut headers = BTreeSet::new();
    for item in items {
        headers.extend(item.keys().cloned());
    }

    let headers_vec = headers.into_iter().collect::<Vec<_>>();
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        let row = headers_vec
            .iter()
            .map(|key| {
                item.get(key)
                    .map(attribute_value_to_string)
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        rows.push(row);
    }

    TableData::new(headers_vec, rows)
}

fn convert_expression_attribute_values(
    raw: &HashMap<String, JsonValue>,
) -> Result<Option<HashMap<String, AttributeValue>>, SendableError> {
    if raw.is_empty() {
        return Ok(None);
    }

    let mut converted = HashMap::new();
    for (key, value) in raw {
        converted.insert(key.clone(), json_to_attribute_value(value)?);
    }

    Ok(Some(converted))
}

fn convert_partiql_parameters(
    raw: Option<&Vec<JsonValue>>,
) -> Result<Option<Vec<AttributeValue>>, SendableError> {
    let Some(values) = raw else {
        return Ok(None);
    };

    if values.is_empty() {
        return Ok(None);
    }

    let converted = values
        .iter()
        .map(json_to_attribute_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(converted))
}

fn json_to_attribute_value(value: &JsonValue) -> Result<AttributeValue, SendableError> {
    match value {
        JsonValue::String(s) => Ok(attr_string(s)),
        JsonValue::Number(number) => Ok(attr_number(number.to_string())),
        JsonValue::Bool(flag) => Ok(attr_bool(*flag)),
        JsonValue::Null => Ok(attr_null(true)),
        JsonValue::Array(values) => {
            let converted = values
                .iter()
                .map(json_to_attribute_value)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(AttributeValue::L(converted))
        }
        JsonValue::Object(map) => object_to_attribute_value(map),
    }
}

fn object_to_attribute_value(
    map: &JsonMap<String, JsonValue>,
) -> Result<AttributeValue, SendableError> {
    let mut normalized = HashMap::new();
    for (key, value) in map {
        normalized.insert(key.to_ascii_uppercase(), value);
    }

    if let Some(raw) = normalized.get("S") {
        if let Some(value) = raw.as_str() {
            return Ok(attr_string(value));
        }
    }

    if let Some(raw) = normalized.get("N") {
        if let Some(value) = raw.as_str() {
            return Ok(attr_number(value));
        }
        if raw.is_number() {
            return Ok(attr_number(raw.to_string()));
        }
    }

    if let Some(raw) = normalized.get("BOOL") {
        if let Some(value) = raw.as_bool() {
            return Ok(attr_bool(value));
        }
    }

    if let Some(raw) = normalized.get("NULL") {
        if let Some(value) = raw.as_bool() {
            return Ok(attr_null(value));
        }
    }

    if let Some(raw) = normalized.get("B") {
        if let Some(value) = raw.as_str() {
            let bytes = BASE64_STANDARD.decode(value).map_err(to_sendable)?;
            return Ok(AttributeValue::B(Blob::new(bytes)));
        }
    }

    if let Some(raw) = normalized.get("SS") {
        if let Some(values) = raw.as_array() {
            let strings = values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| invalid_attribute_value("SS entries must be strings"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(AttributeValue::Ss(strings));
        }
    }

    if let Some(raw) = normalized.get("NS") {
        if let Some(values) = raw.as_array() {
            let numbers = values
                .iter()
                .map(|value| match value {
                    JsonValue::String(s) => Ok(s.to_string()),
                    JsonValue::Number(n) => Ok(n.to_string()),
                    _ => Err(invalid_attribute_value(
                        "NS entries must be numeric strings",
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(AttributeValue::Ns(numbers));
        }
    }

    if let Some(raw) = normalized.get("BS") {
        if let Some(values) = raw.as_array() {
            let blobs = values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| invalid_attribute_value("BS entries must be base64 strings"))
                        .and_then(|s| {
                            BASE64_STANDARD
                                .decode(s)
                                .map_err(to_sendable)
                                .map(|bytes| Blob::new(bytes))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(AttributeValue::Bs(blobs));
        }
    }

    if let Some(raw) = normalized.get("L") {
        if let Some(values) = raw.as_array() {
            let converted = values
                .iter()
                .map(json_to_attribute_value)
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(AttributeValue::L(converted));
        }
    }

    if let Some(raw) = normalized.get("M") {
        if let Some(object) = raw.as_object() {
            let mut converted = HashMap::new();
            for (key, value) in object {
                converted.insert(key.clone(), json_to_attribute_value(value)?);
            }
            return Ok(AttributeValue::M(converted));
        }
    }

    let mut converted = HashMap::new();
    for (key, value) in map {
        converted.insert(key.clone(), json_to_attribute_value(value)?);
    }

    Ok(AttributeValue::M(converted))
}

fn attribute_value_to_string(value: &AttributeValue) -> String {
    match attribute_value_to_json(value) {
        JsonValue::String(s) => s,
        other => other.to_string(),
    }
}

fn attribute_value_to_json(value: &AttributeValue) -> JsonValue {
    if let Ok(text) = value.as_s() {
        return JsonValue::String(text.to_string());
    }
    if let Ok(number) = value.as_n() {
        return JsonValue::String(number.to_string());
    }
    if let Ok(flag) = value.as_bool() {
        return JsonValue::Bool(*flag);
    }
    if value.as_null().map(|flag| *flag).unwrap_or(false) {
        return JsonValue::Null;
    }
    if let Ok(blob) = value.as_b() {
        return JsonValue::String(BASE64_STANDARD.encode(blob.as_ref()));
    }
    if let Ok(values) = value.as_ss() {
        return JsonValue::Array(
            values
                .iter()
                .map(|s| JsonValue::String(s.clone()))
                .collect(),
        );
    }
    if let Ok(values) = value.as_ns() {
        return JsonValue::Array(
            values
                .iter()
                .map(|s| JsonValue::String(s.clone()))
                .collect(),
        );
    }
    if let Ok(values) = value.as_bs() {
        return JsonValue::Array(
            values
                .iter()
                .map(|blob| JsonValue::String(BASE64_STANDARD.encode(blob.as_ref())))
                .collect(),
        );
    }
    if let Ok(values) = value.as_l() {
        return JsonValue::Array(values.iter().map(attribute_value_to_json).collect());
    }
    if let Ok(map) = value.as_m() {
        let mut json_map = JsonMap::new();
        for (key, attr) in map {
            json_map.insert(key.clone(), attribute_value_to_json(attr));
        }
        return JsonValue::Object(json_map);
    }

    JsonValue::Null
}

fn sanitize_limit(limit: Option<i32>) -> Option<i32> {
    limit.filter(|value| *value > 0)
}

fn sanitize_file_stem(input: &str) -> String {
    let mut sanitized = input
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ if ch.is_control() => '_',
            _ => ch,
        })
        .collect::<String>();

    sanitized = sanitized
        .trim()
        .trim_matches('.')
        .trim_matches('\'')
        .to_string();

    const MAX_LEN: usize = 120;
    if sanitized.len() > MAX_LEN {
        sanitized.truncate(MAX_LEN);
    }

    sanitized
}

fn normalize_timeout(timeout_secs: i64) -> Duration {
    if timeout_secs <= 0 {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(timeout_secs as u64)
    }
}

fn attr_string(value: impl Into<String>) -> AttributeValue {
    AttributeValue::S(value.into())
}

fn attr_number(value: impl Into<String>) -> AttributeValue {
    AttributeValue::N(value.into())
}

fn attr_bool(value: bool) -> AttributeValue {
    AttributeValue::Bool(value)
}

fn attr_null(value: bool) -> AttributeValue {
    AttributeValue::Null(value)
}

fn invalid_attribute_value(message: impl Into<String>) -> SendableError {
    Box::new(RuntimeError::new(
        "INVALID_ATTRIBUTE_VALUE".to_string(),
        message.into(),
    ))
}

fn invalid_request(code: &str, message: impl Into<String>) -> SendableError {
    Box::new(RuntimeError::new(code.to_string(), message.into()))
}

fn to_sendable<E>(err: E) -> SendableError
where
    E: Error + Send + Sync + 'static,
{
    Box::new(err)
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DumpFormat {
    Excel,
    Csv,
}

impl Default for DumpFormat {
    fn default() -> Self {
        DumpFormat::Excel
    }
}

impl DumpFormat {
    fn file_extension(&self) -> &'static str {
        match self {
            DumpFormat::Excel => "xlsx",
            DumpFormat::Csv => "csv",
        }
    }

    fn requires_sheet_name(&self) -> bool {
        matches!(self, DumpFormat::Excel)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DynamoQueryType {
    Query,
    Partiql,
}

impl Default for DynamoQueryType {
    fn default() -> Self {
        DynamoQueryType::Query
    }
}

#[derive(Debug, Deserialize)]
struct DynamoDumpRequest {
    table_name: String,
    #[serde(default)]
    index_name: Option<String>,
    #[serde(default)]
    key_condition_expression: Option<String>,
    #[serde(default)]
    filter_expression: Option<String>,
    #[serde(default)]
    projection_expression: Option<String>,
    #[serde(default)]
    expression_attribute_values: HashMap<String, JsonValue>,
    #[serde(default)]
    expression_attribute_names: HashMap<String, String>,
    dump_folder: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    format: DumpFormat,
    #[serde(default)]
    sheet_name: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    limit: Option<i32>,
    #[serde(default)]
    consistent_read: Option<bool>,
    #[serde(default)]
    scan_index_forward: Option<bool>,
    #[serde(default)]
    query_type: DynamoQueryType,
    #[serde(default)]
    partiql_statement: Option<String>,
    #[serde(default)]
    partiql_parameters: Option<Vec<JsonValue>>,
}
