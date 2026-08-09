//! typed program ast for workflow expressions and compute blocks.
//!
//! these are the in-memory typed forms of the `$ref`/`$concat`/`$call`/`$if` expression encoding and
//! the `$let`/`$return`/`$goto`/`$if` compute-statement encoding. they live here (the lowest shared
//! crate) so `WorkflowNode` fields can be typed against them. the structural `Value` <-> ast parse and
//! serialize (validation preserved) live here too; only *evaluation* (ast + context -> value) stays in
//! `runinator-workflows`. the data here carries the *program*; runtime *data* (inputs, outputs, run
//! state) stays dynamic `Value`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::value::{Map, Value};
use crate::workflows::WorkflowNodeRef;

/// one segment of a value-reference path: an object key or an array index.
mod vocabulary;
pub use vocabulary::*;
mod conditions;
pub use conditions::*;
mod conversions;
pub use conversions::*;
