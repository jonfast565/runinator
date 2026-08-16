//! the expression and compute language shared by the workflow graph layer, the reducer, the wdl
//! front end, and `runinator-provider-std`.
//!
//! this crate is everything a caller needs to *evaluate* a value: `$ref`/`$template` resolution,
//! the declarative condition form, the compute program interpreter, the `std` intrinsic library,
//! and user-defined function tables. it knows nothing about workflow graphs — no nodes, no
//! transitions, no validation of a definition. `runinator-workflows` sits on top of it and adds
//! exactly that.
//!
//! the `WORKFLOW` error dictionary is defined here rather than in `runinator-workflows` because
//! both crates emit the same `WorkflowValidationError`; this mirrors how the `WDL` dictionary is
//! single-sourced in `runinator-wdl-syntax` for the four wdl crates.

mod assemble;
mod catalog;
mod compute;
mod conditions;
mod errors;
mod expressions;
mod functions;
mod intrinsic_typing;
mod operators;
mod vm;

/// the declarative-condition wire keys, shared with the graph layer's parameter parsers and type
/// checker so the two cannot spell an operator differently.
pub mod keys;

pub use assemble::{
    CONCAT_INTRINSIC, EXISTS_INTRINSIC, IN_INTRINSIC, IS_NULL_INTRINSIC, NEG_INTRINSIC,
    NOT_INTRINSIC, OPERATOR_INTRINSICS, TO_JSON_INTRINSIC, TO_STRING_INTRINSIC, TRUTHY_INTRINSIC,
    assemble_module, assemble_program,
};
pub use catalog::{
    ArgumentBindError, CallableCatalog, CallableEntry, CallableKind, LOCAL_INTRINSIC_NAMES,
    SECRET_URI_PREFIX, contains_secret_reference, intrinsic_effect,
};
pub use compute::{
    ComputeOutcome, EFFECTFUL_INTRINSIC_NAMES, HIGHER_ORDER_NAMES, IntrinsicLibrary,
    PureIntrinsics, STD_MODULES, STD_NAMESPACE, call_pure, effectful_signatures, intrinsic_arity,
    intrinsic_module, intrinsic_signature, is_higher_order, is_known_intrinsic, parse_program,
    qualified_intrinsic_name, resolve_std_path, run_program, run_program_with,
};
pub use conditions::{
    evaluate_condition, evaluate_condition_with, evaluate_workflow_condition, next_transition,
    validate_condition, validate_condition_value,
};
pub use errors::{WorkflowTypeDiagnostic, WorkflowValidationError};
pub use expressions::{
    apply_input_defaults, evaluate_expression, parse_expression, parse_value_ref,
    resolve_value_refs, resolve_value_refs_pure, resolve_value_refs_with_functions,
    serialize_value_ref, validate_expression,
};
pub use functions::{FunctionTable, RuntimeFunction, intrinsic_catalog};
pub use intrinsic_typing::intrinsic_result_type;
pub use operators::{call_operator, is_operator_intrinsic};
pub use vm::{
    MAX_FRAME_DEPTH, MAX_INSTRUCTIONS_PER_STEP, VmEnv, evaluate_pure, resume, start, step,
};

#[cfg(test)]
mod compute_tests;
#[cfg(test)]
mod functions_tests;
