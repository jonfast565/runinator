use super::*;

#[test]
fn static_locator_resolves_sync() {
    let locator = StaticLocator::new("http://localhost:8080");
    assert_eq!(
        BlockingServiceLocator::wait_for_service_url(&locator).unwrap(),
        "http://localhost:8080"
    );
}
