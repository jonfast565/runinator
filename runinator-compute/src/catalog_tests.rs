//! covers the unified callable catalog: what it knows, how it classifies effects, and how it binds
//! named arguments.

use super::*;
use runinator_models::providers::{ParameterMetadata, ProviderRuntimeMetadata};
use runinator_models::types::RuninatorType;
use runinator_models::value::Value;

fn provider(name: &str, actions: Vec<ActionMetadata>) -> ProviderMetadata {
    ProviderMetadata {
        name: name.to_string(),
        actions,
        metadata: ProviderRuntimeMetadata::default(),
    }
}

#[test]
fn the_builtin_catalog_covers_every_intrinsic_list() {
    let catalog = CallableCatalog::builtin();
    // the three lists that used to be maintained separately must all be present.
    for name in PureIntrinsics::names() {
        assert!(catalog.knows(name), "missing pure intrinsic {name}");
    }
    for name in EFFECTFUL_INTRINSIC_NAMES {
        assert!(catalog.knows(name), "missing effectful intrinsic {name}");
    }
    for name in HIGHER_ORDER_NAMES {
        assert!(catalog.knows(name), "missing higher-order intrinsic {name}");
    }
}

#[test]
fn now_uuid_and_env_are_local_not_durable() {
    let catalog = CallableCatalog::builtin();
    for name in LOCAL_INTRINSIC_NAMES {
        assert_eq!(
            catalog.effect_of(name),
            EffectClass::Local,
            "{name} should be local"
        );
        // local calls fold in the reducer rather than costing a broker round trip.
        assert!(catalog.resolve(name).expect("entry").is_in_process());
    }
}

#[test]
fn http_intrinsics_stay_durable() {
    let catalog = CallableCatalog::builtin();
    for name in ["http_get", "http_post"] {
        assert_eq!(catalog.effect_of(name), EffectClass::Durable);
        assert!(!catalog.resolve(name).expect("entry").is_in_process());
    }
}

#[test]
fn ordinary_intrinsics_are_pure() {
    let catalog = CallableCatalog::builtin();
    for name in ["upper", "len", "merge", "regex_match"] {
        assert_eq!(catalog.effect_of(name), EffectClass::Pure, "{name}");
    }
}

#[test]
fn an_unknown_name_is_unknown_rather_than_pure() {
    let catalog = CallableCatalog::builtin();
    // refusing to guess is what stops a typo being folded in the reducer.
    assert_eq!(catalog.effect_of("nope"), EffectClass::Unknown);
    assert!(!catalog.knows("nope"));
}

#[test]
fn provider_actions_are_durable_and_namespaced() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_provider(&provider(
        "github",
        vec![ActionMetadata::new("deploy", "deploy something")],
    ));
    assert!(catalog.knows("github.deploy"));
    assert_eq!(catalog.effect_of("github.deploy"), EffectClass::Durable);
    // the bare action name is not in scope; only the qualified surface name is.
    assert!(!catalog.knows("deploy"));
}

#[test]
fn a_local_function_carries_the_effect_the_compiler_computed() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_local("pure_helper", 1, EffectClass::Pure);
    catalog.add_local("calls_http", 0, EffectClass::Durable);
    assert_eq!(catalog.effect_of("pure_helper"), EffectClass::Pure);
    assert_eq!(catalog.effect_of("calls_http"), EffectClass::Durable);
    assert!(matches!(
        catalog.resolve("pure_helper").expect("entry").kind,
        CallableKind::Local
    ));
}

#[test]
fn a_packaged_export_is_durable_and_keeps_its_binding() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_packaged(binding(), None);
    let entry = catalog
        .resolve("functions.image_tools.resize")
        .expect("entry");
    assert_eq!(entry.effect, EffectClass::Durable);
    let target = entry.target();
    assert_eq!(target.binding().expect("binding").version, 4);
}

fn binding() -> FunctionBinding {
    FunctionBinding {
        package_id: uuid::Uuid::nil(),
        package_name: "image_tools".to_string(),
        namespace: None,
        version_id: uuid::Uuid::nil(),
        version: 4,
        export_id: uuid::Uuid::nil(),
        export_name: "resize".to_string(),
        artifact_digest: "sha256:abc".to_string(),
    }
}

#[test]
fn entries_compile_to_the_matching_invocation_target() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_local("helper", 0, EffectClass::Pure);
    catalog.add_provider(&provider(
        "slack",
        vec![ActionMetadata::new("post", "post a message")],
    ));
    assert!(matches!(
        catalog.resolve("upper").expect("e").target(),
        CallableTarget::Intrinsic { .. }
    ));
    assert!(matches!(
        catalog.resolve("helper").expect("e").target(),
        CallableTarget::Local { .. }
    ));
    assert!(matches!(
        catalog.resolve("slack.post").expect("e").target(),
        CallableTarget::Provider { .. }
    ));
}

#[test]
fn arity_comes_from_the_intrinsic_table() {
    let catalog = CallableCatalog::builtin();
    let upper = catalog.resolve("upper").expect("entry");
    assert!(upper.accepts_argc(1));
    assert!(!upper.accepts_argc(2));
    // `now` takes nothing at all.
    assert!(catalog.resolve("now").expect("entry").accepts_argc(0));
    assert!(!catalog.resolve("now").expect("entry").accepts_argc(1));
}

#[test]
fn named_arguments_bind_into_signature_order() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_provider(&provider(
        "svc",
        vec![ActionMetadata::new("call", "a call").with_parameters(vec![
            ParameterMetadata::required("first", RuninatorType::String),
            ParameterMetadata::optional("second", RuninatorType::String),
        ])],
    ));
    // written out of order, bound into declaration order.
    let bound = catalog
        .bind_arguments(
            "svc.call",
            &[],
            &[
                ("second".to_string(), Value::from("b")),
                ("first".to_string(), Value::from("a")),
            ],
        )
        .expect("bind");
    assert_eq!(bound, vec![Value::from("a"), Value::from("b")]);
}

#[test]
fn positional_and_named_arguments_combine() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_provider(&provider(
        "svc",
        vec![ActionMetadata::new("call", "a call").with_parameters(vec![
            ParameterMetadata::required("first", RuninatorType::String),
            ParameterMetadata::optional("second", RuninatorType::String),
        ])],
    ));
    let bound = catalog
        .bind_arguments(
            "svc.call",
            &[Value::from("a")],
            &[("second".to_string(), Value::from("b"))],
        )
        .expect("bind");
    assert_eq!(bound, vec![Value::from("a"), Value::from("b")]);
}

#[test]
fn an_undeclared_parameter_name_is_rejected() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_provider(&provider(
        "svc",
        vec![ActionMetadata::new("call", "a call").with_parameters(vec![
            ParameterMetadata::required("first", RuninatorType::String),
        ])],
    ));
    let err = catalog
        .bind_arguments("svc.call", &[], &[("nope".to_string(), Value::from("x"))])
        .expect_err("should reject");
    assert!(matches!(err, ArgumentBindError::UnknownParameter { .. }));
}

#[test]
fn supplying_one_parameter_twice_is_rejected() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_provider(&provider(
        "svc",
        vec![ActionMetadata::new("call", "a call").with_parameters(vec![
            ParameterMetadata::required("first", RuninatorType::String),
        ])],
    ));
    let err = catalog
        .bind_arguments(
            "svc.call",
            &[Value::from("a")],
            &[("first".to_string(), Value::from("b"))],
        )
        .expect_err("should reject");
    assert!(matches!(err, ArgumentBindError::DuplicateParameter { .. }));
}

#[test]
fn a_gap_before_a_supplied_argument_is_rejected() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_provider(&provider(
        "svc",
        vec![ActionMetadata::new("call", "a call").with_parameters(vec![
            ParameterMetadata::required("first", RuninatorType::String),
            ParameterMetadata::required("second", RuninatorType::String),
        ])],
    ));
    // `second` given, `first` omitted: there is no positional form for that.
    let err = catalog
        .bind_arguments("svc.call", &[], &[("second".to_string(), Value::from("b"))])
        .expect_err("should reject");
    assert!(matches!(err, ArgumentBindError::MissingParameter { .. }));
}

#[test]
fn binding_an_unknown_callable_is_rejected() {
    let catalog = CallableCatalog::builtin();
    let err = catalog
        .bind_arguments("nope", &[Value::from(1i64)], &[])
        .expect_err("should reject");
    assert!(matches!(err, ArgumentBindError::UnknownCallable(_)));
}

#[test]
fn a_secret_placeholder_is_detected_anywhere_in_a_value() {
    // this is the guard that keeps the reducer from computing over a secret's placeholder text.
    assert!(contains_secret_reference(&Value::from("secret://aws/key")));
    assert!(contains_secret_reference(&Value::Array(vec![
        Value::from("plain"),
        Value::from("secret://aws/key"),
    ])));
    let mut map = runinator_models::value::Map::new();
    map.insert("k".to_string(), Value::from("secret://aws/key"));
    assert!(contains_secret_reference(&Value::Object(map)));
}

#[test]
fn an_ordinary_value_holds_no_secret() {
    assert!(!contains_secret_reference(&Value::from("plain")));
    assert!(!contains_secret_reference(&Value::from(1i64)));
    assert!(!contains_secret_reference(&Value::Null));
    // a string merely mentioning the word is not a reference.
    assert!(!contains_secret_reference(&Value::from("my secret value")));
}
