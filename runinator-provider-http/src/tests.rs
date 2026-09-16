use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

#[test]
fn metadata_exposes_one_request_action() {
    let metadata = HttpProvider.metadata();
    assert_eq!(metadata.name, "http");
    assert_eq!(metadata.actions.len(), 1);
    assert_eq!(metadata.actions[0].function_name, "request");
}

#[test]
fn status_policy_defaults_to_success_and_can_accept_anything() {
    assert!(status_expected(204, None));
    assert!(!status_expected(404, None));
    assert!(status_expected(404, Some(&[])));
    assert!(status_expected(404, Some(&[200, 404])));
}

#[test]
fn private_targets_need_an_explicit_allowlist() {
    let url = Url::parse("http://127.0.0.1:8080/test").unwrap();
    assert!(validate_target(&url, &BTreeSet::new()).is_err());
    assert!(validate_target(&url, &BTreeSet::from(["127.0.0.1".into()])).is_ok());
}

#[test]
fn binary_response_is_base64_encoded() {
    let value = decode_response_body("application/octet-stream", &[0, 159, 146, 150]).unwrap();
    assert_eq!(value["encoding"], "base64");
}

#[test]
fn authenticated_put_returns_a_non_success_status_as_a_value_when_requested() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let read = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.starts_with("PUT /items/7?dry_run=true HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer secret-value")
        );
        stream
            .write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 17\r\nConnection: close\r\n\r\n{\"missing\":true}\n",
            )
            .unwrap();
    });
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "http".into(),
        action_function: "request".into(),
        parameters: Value::Null,
        timeout_secs: 5,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: runinator_models::runs::MaterializedCredentialInjections {
            headers: BTreeMap::from([("Authorization".into(), "Bearer secret-value".into())]),
            ..Default::default()
        },
    };
    let result = execute_request_with_allowed_hosts(
        &request,
        RequestParams {
            method: "PUT".into(),
            url: format!("http://{address}/items/7"),
            headers: BTreeMap::new(),
            query: BTreeMap::from([("dry_run".into(), "true".into())]),
            body: Some(json!({ "name": "demo" })),
            body_format: "json".into(),
            timeout_seconds: None,
            follow_redirects: false,
            expect_status: Some(vec![404]),
        },
        BTreeSet::from(["127.0.0.1".into()]),
    )
    .unwrap();
    server.join().unwrap();
    let output = result.output_json.unwrap();
    assert_eq!(output["response"]["status"], 404);
    assert_eq!(output["response"]["body"]["missing"], true);
}
