use super::*;

fn mock_workflow(id: i64, name: &str, enabled: bool) -> WorkflowDefinition {
    WorkflowDefinition {
        id: Some(id),
        name: name.into(),
        version: 1,
        enabled,
        input_type: runinator_models::types::RuninatorType::from_json_schema(
            &json!({ "type": "object" }),
        ),
        definition: runinator_models::workflows::WorkflowGraph::default(),
        created_at: None,
        updated_at: None,
    }
}

fn spawn_resource_list_api() -> (String, thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        for _ in 0..7 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }

            let path = request_line.split_whitespace().nth(1).unwrap_or("");
            let body = if path == "/workflow_runs" {
                json!([{ "id": 12, "workflow_id": 3, "status": "running" }])
            } else if path.starts_with("/runs?status=") {
                json!([])
            } else {
                panic!("unexpected path {path}");
            };
            let body = body.to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    (format!("http://{address}"), handle)
}

#[test]
fn tools_include_only_enabled_workflows() {
    let tools = tools_from_workflows(vec![
        mock_workflow(1, "Allowed", true),
        mock_workflow(2, "Disabled", false),
    ]);

    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("name").and_then(Value::as_str),
        Some("allowed_1")
    );
}

#[test]
fn fixed_tools_include_workflow_authoring_surface() {
    let tools = fixed_tools();
    let names = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert!(names.contains(&"runinator_list_providers"));
    assert!(names.contains(&"runinator_validate_workflow"));
    assert!(names.contains(&"runinator_export_workflow_bundle"));
}

#[test]
fn fixed_tool_names_do_not_parse_as_workflow_ids() {
    assert_eq!(parse_tool_workflow_id("runinator_list_providers"), None);
    assert_eq!(parse_tool_workflow_id("build_pipeline_42"), Some(42));
}

#[test]
fn export_response_includes_structured_content_and_json_text() {
    let bundle = json!({
        "workflows": [{ "id": 1, "name": "demo" }],
        "triggers": [],
    });

    let response = json_export_response(bundle.clone()).unwrap();

    assert_eq!(response["structuredContent"], bundle);
    assert!(
        response["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"workflows\"")
    );
}

#[test]
fn workflow_resources_map_to_api_paths() {
    assert_eq!(
        resource_path_for_uri("runinator://workflows").as_deref(),
        Some("workflows")
    );
    assert_eq!(
        resource_path_for_uri("runinator://workflows/7").as_deref(),
        Some("workflows/7")
    );
    assert_eq!(
        resource_path_for_uri("runinator://workflow_runs/12").as_deref(),
        Some("workflow_runs/12")
    );
}

#[test]
fn admin_mutation_methods_are_rejected() {
    let server = McpServer::new("http://127.0.0.1:9/".into()).unwrap();
    let response = server.handle(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tasks/create",
        "params": {}
    }));

    assert!(response.get("error").is_some());
}

#[test]
fn resource_entries_include_run_children() {
    let entries = resource_entries_from_runs(&[json!({ "id": 7, "status": "succeeded" })]);
    let uris = entries
        .iter()
        .filter_map(|entry| entry.get("uri").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert!(uris.contains(&"runinator://runs/7"));
    assert!(uris.contains(&"runinator://runs/7/chunks"));
    assert!(uris.contains(&"runinator://runs/7/artifacts"));
}

#[test]
fn resources_list_includes_recent_workflow_runs() {
    let (base_url, handle) = spawn_resource_list_api();
    let server = McpServer::new(base_url).unwrap();

    let response = server.resources_list().unwrap();
    handle.join().unwrap();

    let uris = response["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry.get("uri").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert!(uris.contains(&"runinator://workflow_runs/12"));
}

#[test]
fn resource_entries_include_workflow_runs() {
    let entries = resource_entries_from_workflow_runs(&[json!({
        "id": 12,
        "workflow_id": 3,
        "status": "running"
    })]);

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].get("uri").and_then(Value::as_str),
        Some("runinator://workflow_runs/12")
    );
    assert_eq!(
        entries[0].get("name").and_then(Value::as_str),
        Some("Workflow run 12: running")
    );
}
