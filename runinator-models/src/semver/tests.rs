use super::*;

#[test]
fn parses_full_and_short_forms() {
    assert_eq!("1.2.3".parse::<SemVer>().unwrap(), SemVer::new(1, 2, 3));
    assert_eq!("2".parse::<SemVer>().unwrap(), SemVer::new(2, 0, 0));
    assert_eq!("1.4".parse::<SemVer>().unwrap(), SemVer::new(1, 4, 0));
}

#[test]
fn rejects_malformed() {
    assert!("".parse::<SemVer>().is_err());
    assert!("1.2.3.4".parse::<SemVer>().is_err());
    assert!("a.b".parse::<SemVer>().is_err());
}

#[test]
fn bumps_reset_lower_components() {
    let base = SemVer::new(1, 2, 3);
    assert_eq!(base.bump(SemVerBump::Major), SemVer::new(2, 0, 0));
    assert_eq!(base.bump(SemVerBump::Minor), SemVer::new(1, 3, 0));
    assert_eq!(base.bump(SemVerBump::Patch), SemVer::new(1, 2, 4));
}

#[test]
fn orders_numerically_not_lexically() {
    assert!(SemVer::new(9, 0, 0) < SemVer::new(10, 0, 0));
    assert!(SemVer::new(1, 2, 0) < SemVer::new(1, 10, 0));
}

#[test]
fn serializes_as_string_and_accepts_legacy_integer() {
    let version = SemVer::new(1, 2, 0);
    assert_eq!(serde_json::to_string(&version).unwrap(), "\"1.2.0\"");
    let from_string: SemVer = serde_json::from_str("\"3.1.4\"").unwrap();
    assert_eq!(from_string, SemVer::new(3, 1, 4));
    let from_legacy_int: SemVer = serde_json::from_str("5").unwrap();
    assert_eq!(from_legacy_int, SemVer::new(5, 0, 0));
}
