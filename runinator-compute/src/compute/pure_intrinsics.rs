#[allow(unused_imports)]
use super::*;

pub struct PureIntrinsics;

impl PureIntrinsics {
    /// the names of every pure intrinsic, in stable order.
    pub fn names() -> &'static [&'static str] {
        &[
            // numeric.
            "add",
            "sub",
            "mul",
            "div",
            "mod",
            "floor",
            "ceil",
            "round",
            "min",
            "max",
            "parse_int",
            "parse_float",
            // strings.
            "lower",
            "upper",
            "trim",
            "split",
            "join",
            "replace",
            "substring",
            "starts_with",
            "ends_with",
            // collections.
            "len",
            "keys",
            "values",
            "contains",
            "at",
            "has",
            "sum",
            "sort",
            "reverse",
            "unique",
            "flatten",
            "slice",
            "first",
            "last",
            "append",
            "range",
            // objects.
            "merge",
            "pick",
            "omit",
            "entries",
            "from_entries",
            // encoding.
            "parse_json",
            "base64_encode",
            "base64_decode",
            "hash",
            "hash_percent",
            // logic / comparison.
            "eq",
            "ne",
            "gt",
            "lt",
            "gte",
            "lte",
            "not",
            "and",
            "or",
            "default",
            // dates.
            "format_date",
            "parse_date",
            "add_duration",
            "date_diff",
            // regex.
            "regex_match",
            "regex_replace",
            "regex_extract",
        ]
    }

    /// whether `name` is a pure intrinsic.
    pub fn contains(name: &str) -> bool {
        Self::names().contains(&name)
    }

    /// typed signatures for the pure intrinsics, used to build provider metadata.
    pub fn signatures() -> Vec<ActionMetadata> {
        let numeric = |name: &str| {
            ActionMetadata::new(name, format!("pure numeric intrinsic {name}"))
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Number),
                    ParameterMetadata::required("b", RuninatorType::Number),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Number)])
                .pure()
        };
        let unary_number_to_int = |name: &str| {
            ActionMetadata::new(name, format!("pure intrinsic {name}"))
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::Number,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
                .pure()
        };
        let unary_string = |name: &str| {
            ActionMetadata::new(name, format!("pure string intrinsic {name}"))
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure()
        };
        // two opaque operands yielding a boolean (comparison/membership predicates).
        let any_predicate = |name: &str| {
            ActionMetadata::new(name, format!("pure predicate intrinsic {name}"))
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("b", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Boolean)])
                .pure()
        };
        // a whole-array transform yielding an array.
        let array_transform = |name: &str| {
            ActionMetadata::new(name, format!("pure collection intrinsic {name}"))
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure()
        };
        vec![
            numeric("add"),
            numeric("sub"),
            numeric("mul"),
            numeric("div"),
            numeric("mod"),
            ActionMetadata::new("len", "length of a string, array, or object")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
                .pure(),
            ActionMetadata::new("keys", "keys of an object")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::String),
                )])
                .pure(),
            unary_string("lower"),
            unary_string("upper"),
            unary_number_to_int("floor"),
            unary_number_to_int("ceil"),
            unary_number_to_int("round"),
            numeric("min"),
            numeric("max"),
            ActionMetadata::new("parse_int", "parse an integer from a string")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
                .pure(),
            ActionMetadata::new("parse_float", "parse a number from a string")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Number)])
                .pure(),
            // strings.
            unary_string("trim"),
            ActionMetadata::new("split", "split a string into parts on a separator")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("sep", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::String),
                )])
                .pure(),
            ActionMetadata::new("join", "join an array into a string with a separator")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("sep", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new("replace", "replace all occurrences of a substring")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("from", RuninatorType::String),
                    ParameterMetadata::required("to", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new("substring", "slice a string by character index")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("start", RuninatorType::Integer),
                    ParameterMetadata::optional("end", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            any_predicate("starts_with"),
            any_predicate("ends_with"),
            // collections.
            ActionMetadata::new("values", "values of an object")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            any_predicate("contains"),
            any_predicate("has"),
            ActionMetadata::new("at", "element at an array index or object key")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("key", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("sum", "sum of a numeric array")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Number)])
                .pure(),
            array_transform("sort"),
            array_transform("reverse"),
            array_transform("unique"),
            array_transform("flatten"),
            ActionMetadata::new("slice", "slice an array by index range")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("start", RuninatorType::Integer),
                    ParameterMetadata::optional("end", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            ActionMetadata::new("first", "first element of an array")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("last", "last element of an array")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("append", "append an element to an array")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("item", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            ActionMetadata::new("range", "an integer range [start, end)")
                .with_parameters(vec![
                    ParameterMetadata::required("start", RuninatorType::Integer),
                    ParameterMetadata::required("end", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Integer),
                )])
                .pure(),
            // objects.
            // the catalog result is `any`; `intrinsic_result_type` recovers a merged struct when
            // both operands are structs, otherwise the shape is not statically known.
            ActionMetadata::new("merge", "shallow-merge two objects (right wins)")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("b", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("pick", "keep only the named keys of an object")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required(
                        "keys",
                        RuninatorType::array(RuninatorType::String),
                    ),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("omit", "drop the named keys of an object")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required(
                        "keys",
                        RuninatorType::array(RuninatorType::String),
                    ),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("entries", "an object as an array of {key, value} pairs")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            // `any`: the object shape is built from runtime keys, not derivable from arg types.
            ActionMetadata::new("from_entries", "build an object from {key, value} pairs")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            // encoding.
            // `any`: the parsed shape is only known at runtime, so the result stays opaque.
            ActionMetadata::new("parse_json", "parse a JSON string into a value")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            unary_string("base64_encode"),
            unary_string("base64_decode"),
            ActionMetadata::new(
                "hash",
                "stable non-negative 63-bit hash of any value (deterministic across processes)",
            )
            .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
            .pure(),
            ActionMetadata::new(
                "hash_percent",
                "stable hash bucket of any value in 0..=99 (for percentage rollouts)",
            )
            .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
            .pure(),
            // logic / comparison.
            any_predicate("eq"),
            any_predicate("ne"),
            any_predicate("gt"),
            any_predicate("lt"),
            any_predicate("gte"),
            any_predicate("lte"),
            ActionMetadata::new("not", "logical negation of a boolean")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::Boolean,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Boolean)])
                .pure(),
            any_predicate("and"),
            any_predicate("or"),
            ActionMetadata::new(
                "default",
                "the first argument when non-null, else the second",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::Any),
                ParameterMetadata::required("b", RuninatorType::Any),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
            .pure(),
            // dates.
            ActionMetadata::new("format_date", "format a timestamp with a strftime pattern")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("fmt", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new(
                "parse_date",
                "parse a timestamp into RFC 3339 with a pattern",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::String),
                ParameterMetadata::required("fmt", RuninatorType::String),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
            .pure(),
            ActionMetadata::new("add_duration", "add seconds to an RFC 3339 timestamp")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("seconds", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new(
                "date_diff",
                "seconds between two RFC 3339 timestamps (a - b)",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::String),
                ParameterMetadata::required("b", RuninatorType::String),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
            .pure(),
            // regex.
            ActionMetadata::new(
                "regex_match",
                "whether a pattern matches anywhere in a string",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::String),
                ParameterMetadata::required("pattern", RuninatorType::String),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Boolean)])
            .pure(),
            ActionMetadata::new("regex_replace", "replace all pattern matches in a string")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("pattern", RuninatorType::String),
                    ParameterMetadata::required("replacement", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new("regex_extract", "all full matches of a pattern in a string")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("pattern", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::String),
                )])
                .pure(),
        ]
    }
}

impl IntrinsicLibrary for PureIntrinsics {
    fn call(&self, name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError> {
        call_pure(name, args)
    }

    fn knows(&self, name: &str) -> bool {
        Self::contains(name)
    }

    fn is_pure(&self, _name: &str) -> bool {
        true
    }
}
