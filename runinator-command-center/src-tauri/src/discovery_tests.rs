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
