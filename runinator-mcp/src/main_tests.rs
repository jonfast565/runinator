use super::*;
use uuid::Uuid;

fn mock_workflow(id: Uuid, name: &str, enabled: bool) -> WorkflowDefinition {
    WorkflowDefinition {
        id: Some(id),
        name: name.into(),
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled,
        input_type: runinator_models::types::RuninatorType::from_json_schema(
            &json!({ "type": "object" }),
        ),
        definition: runinator_models::workflows::WorkflowGraph::default(),
        created_at: None,
        updated_at: None,
    }
}

fn spawn_resource_list_api(run_id: Uuid) -> (String, thread::JoinHandle<()>) {
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
                json!([{ "id": run_id.to_string(), "workflow_id": Uuid::now_v7().to_string(), "status": "running" }])
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
    let allowed_id = Uuid::now_v7();
    let tools = tools_from_workflows(vec![
        mock_workflow(allowed_id, "Allowed", true),
        mock_workflow(Uuid::now_v7(), "Disabled", false),
    ]);

    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0].get("name").and_then(Value::as_str),
        Some(format!("allowed_{allowed_id}").as_str())
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
    // a non-uuid suffix is not a workflow id.
    assert_eq!(parse_tool_workflow_id("build_pipeline_42"), None);
    // a uuid suffix is parsed back out of the generated tool name.
    let id = Uuid::now_v7();
    assert_eq!(
        parse_tool_workflow_id(&format!("build_pipeline_{id}")),
        Some(id)
    );
}

#[test]
fn export_response_includes_structured_content_and_json_text() {
    let bundle = json!({
        "workflows": [{ "id": Uuid::now_v7().to_string(), "name": "demo" }],
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
    let workflow_id = Uuid::now_v7();
    let workflow_run_id = Uuid::now_v7();
    assert_eq!(
        resource_path_for_uri("runinator://workflows").as_deref(),
        Some("workflows")
    );
    assert_eq!(
        resource_path_for_uri(&format!("runinator://workflows/{workflow_id}")).as_deref(),
        Some(format!("workflows/{workflow_id}").as_str())
    );
    assert_eq!(
        resource_path_for_uri(&format!("runinator://workflow_runs/{workflow_run_id}")).as_deref(),
        Some(format!("workflow_runs/{workflow_run_id}").as_str())
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
    let run_id = Uuid::now_v7();
    let entries =
        resource_entries_from_runs(&[json!({ "id": run_id.to_string(), "status": "succeeded" })]);
    let uris = entries
        .iter()
        .filter_map(|entry| entry.get("uri").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert!(uris.contains(&format!("runinator://runs/{run_id}").as_str()));
    assert!(uris.contains(&format!("runinator://runs/{run_id}/chunks").as_str()));
    assert!(uris.contains(&format!("runinator://runs/{run_id}/artifacts").as_str()));
}

#[test]
fn resources_list_includes_recent_workflow_runs() {
    let run_id = Uuid::now_v7();
    let (base_url, handle) = spawn_resource_list_api(run_id);
    let server = McpServer::new(base_url).unwrap();

    let response = server.resources_list().unwrap();
    handle.join().unwrap();

    let uris = response["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry.get("uri").and_then(Value::as_str))
        .collect::<Vec<_>>();

    assert!(uris.contains(&format!("runinator://workflow_runs/{run_id}").as_str()));
}

#[test]
fn resource_entries_include_workflow_runs() {
    let run_id = Uuid::now_v7();
    let entries = resource_entries_from_workflow_runs(&[json!({
        "id": run_id.to_string(),
        "workflow_id": Uuid::now_v7().to_string(),
        "status": "running"
    })]);

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].get("uri").and_then(Value::as_str),
        Some(format!("runinator://workflow_runs/{run_id}").as_str())
    );
    assert_eq!(
        entries[0].get("name").and_then(Value::as_str),
        Some(format!("Workflow run {run_id}: running").as_str())
    );
}
