use super::*;

#[test]
fn service_url_includes_base_path_and_trailing_slash() {
    let service = WebServiceAnnouncement {
        service_id: "svc".into(),
        address: "127.0.0.1".into(),
        port: 8080,
        base_path: "api".into(),
        last_heartbeat: Utc::now(),
    };
    assert_eq!(
        build_service_base_url(&service),
        "http://127.0.0.1:8080/api/"
    );
}

#[test]
fn announcement_falls_back_to_sender_and_service_id() {
    let payload =
        br#"{"type":"web_service","service":{"address":"","port":8080,"base_path":"/api"}}"#;
    let service = parse_announcement(payload, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))).unwrap();
    assert_eq!(service.address, "10.0.0.5");
    assert_eq!(service.service_id, "10.0.0.5:8080");
    assert_eq!(
        build_service_base_url(&service),
        "http://10.0.0.5:8080/api/"
    );
}

#[test]
fn configured_service_url_is_normalized_for_api_paths() {
    assert_eq!(
        normalize_configured_service_url(
            "https://runinator.example.test/api?ignored=true#fragment"
        )
        .unwrap(),
        "https://runinator.example.test/api/"
    );
}

#[test]
fn configured_service_url_uses_first_non_empty_value() {
    let pairs = vec![
        (
            "RUNINATOR_COMMAND_CENTER_SERVICE_URL".to_string(),
            " ".to_string(),
        ),
        (
            "RUNINATOR_SERVICE_URL".to_string(),
            "http://127.0.0.1:8080/api".to_string(),
        ),
        (
            "WS_API_BASE_URL".to_string(),
            "http://runinator-ws.runinator.svc.cluster.local:8080/".to_string(),
        ),
    ];

    assert_eq!(
        configured_service_url_from_pairs(pairs).unwrap(),
        Some("http://127.0.0.1:8080/api/".to_string())
    );
}

#[test]
fn configured_service_url_rejects_non_http_urls() {
    let pairs = vec![(
        "RUNINATOR_COMMAND_CENTER_SERVICE_URL".to_string(),
        "file:///tmp/runinator.sock".to_string(),
    )];

    let err = configured_service_url_from_pairs(pairs).unwrap_err();
    assert!(err.contains("RUNINATOR_COMMAND_CENTER_SERVICE_URL"));
    assert!(err.contains("http or https"));
}
