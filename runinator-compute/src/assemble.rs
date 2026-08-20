//! assemble a `ComputeProgram` into an [`InvocationProgram`].
//!
//! this is the bridge between the two program forms: the statement/expression tree the rexrap compiler
//! already emits, and the flat instruction stream the vm runs. it lives here rather than in the rexrap
//! crates because it has two callers with nothing else in common — the compiler, which assembles
//! what it just lowered, and the migration, which assembles definitions that were compiled years
//! ago and have no rexrap source to re-lower.
//!
//! assembling from the *lowered* tree rather than from the rexrap ast is deliberate. it means the
//! expression surface has exactly one lowering (`$ref`, `$concat`, `$call`, `$lambda`, …), so a new
//! surface form cannot reach the vm without going through the same json the evaluator sees; and it
//! is what lets a stored definition be converted without the compiler that produced it.
//!
//! two forms stay lazy here rather than becoming eager calls, because their laziness is load-bearing
//! rather than an optimization: `$if` (a recursive function's base case must be able to terminate
//! before its recursive branch is evaluated) and `$coalesce` (whose right-hand side is routinely the
//! expensive or recursive one). both compile to jumps.

use runinator_models::invocation::{
    CallableTarget, InvocationFunction, InvocationInstruction, InvocationModule, InvocationProgram,
};
use runinator_models::value::Value;
use runinator_models::workflow_ast::{
    CompareOp, ComputeProgram, ComputeStmt, ConditionNode, WorkflowExpression,
};

use crate::catalog::CallableCatalog;
use crate::errors::WorkflowValidationError;

/// assemble one compute program into a module with no functions.
pub fn assemble_program(
    program: &ComputeProgram,
    catalog: &CallableCatalog,
) -> Result<InvocationProgram, WorkflowValidationError> {
    let mut out = Assembler::new(catalog);
    out.block(&program.0)?;
    Ok(InvocationProgram::new(out.instructions))
}

/// Assemble one declarative expression into a program that returns its value.
pub(crate) fn assemble_expression(
    expression: &WorkflowExpression,
    catalog: &CallableCatalog,
) -> Result<InvocationProgram, WorkflowValidationError> {
    let mut out = Assembler::new(catalog);
    out.expression(expression)?;
    out.emit(InvocationInstruction::Return);
    Ok(InvocationProgram::new(out.instructions))
}

/// Assemble one declarative condition into a program that returns its boolean result.
pub(crate) fn assemble_condition(
    condition: &ConditionNode,
    catalog: &CallableCatalog,
) -> Result<InvocationProgram, WorkflowValidationError> {
    let mut out = Assembler::new(catalog);
    out.condition(condition)?;
    out.emit(InvocationInstruction::Return);
    Ok(InvocationProgram::new(out.instructions))
}

/// assemble a program plus the user functions it may call into a complete module.
pub fn assemble_module(
    program: &ComputeProgram,
    functions: &[(String, Vec<String>, ComputeProgram, Option<u32>)],
    catalog: &CallableCatalog,
) -> Result<InvocationModule, WorkflowValidationError> {
    let entry = assemble_program(program, catalog)?;
    let mut module = InvocationModule::new(entry);
    for (name, params, body, max_depth) in functions {
        module.functions.push(InvocationFunction {
            name: name.clone(),
            params: params.clone(),
            body: assemble_program(body, catalog)?,
            max_depth: *max_depth,
        });
    }
    Ok(module)
}

struct Assembler<'a> {
    instructions: Vec<InvocationInstruction>,
    catalog: &'a CallableCatalog,
}

impl<'a> Assembler<'a> {
    fn new(catalog: &'a CallableCatalog) -> Self {
        Self {
            instructions: Vec::new(),
            catalog,
        }
    }

    fn emit(&mut self, instruction: InvocationInstruction) -> usize {
        self.instructions.push(instruction);
        self.instructions.len() - 1
    }

    fn here(&self) -> usize {
        self.instructions.len()
    }

    /// rewrite a jump placeholder to land at the current position.
    fn patch(&mut self, at: usize) {
        let target = self.here();
        match &mut self.instructions[at] {
            InvocationInstruction::Jump { target: slot }
            | InvocationInstruction::JumpIfFalse { target: slot } => *slot = target,
            other => unreachable!("patched a non-jump instruction: {other:?}"),
        }
    }

    fn block(&mut self, statements: &[ComputeStmt]) -> Result<(), WorkflowValidationError> {
        for statement in statements {
            self.statement(statement)?;
        }
        Ok(())
    }

    fn statement(&mut self, statement: &ComputeStmt) -> Result<(), WorkflowValidationError> {
        match statement {
            ComputeStmt::Let { name, value } => {
                self.expression(value)?;
                self.emit(InvocationInstruction::StoreLocal { name: name.clone() });
            }
            ComputeStmt::Return(expression) => {
                self.expression(expression)?;
                self.emit(InvocationInstruction::Return);
            }
            ComputeStmt::Goto(target) => {
                self.emit(InvocationInstruction::Goto {
                    target: target.clone(),
                });
            }
            ComputeStmt::Expr(expression) => {
                // a bare expression statement is evaluated for its effect and discarded. the
                // evaluator agrees: falling off the end of a block yields null, not the last value.
                self.expression(expression)?;
                self.emit(InvocationInstruction::Pop);
            }
            ComputeStmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.condition(condition)?;
                let to_else = self.emit(InvocationInstruction::JumpIfFalse { target: 0 });
                self.block(&then_branch.0)?;
                let over_else = self.emit(InvocationInstruction::Jump { target: 0 });
                self.patch(to_else);
                self.block(&else_branch.0)?;
                self.patch(over_else);
            }
        }
        Ok(())
    }

    /// compile a declarative condition into instructions that leave a boolean on the stack.
    ///
    /// structurally rather than by handing the whole node to an intrinsic, because an intrinsic
    /// would need the run context to resolve the refs inside it — and the only thing in the vm that
    /// has the context is the `LoadRef` instruction. compiling it out means conditions resolve
    /// through exactly the same path every other reference does.
    ///
    /// `all`/`any` short-circuit, matching the evaluator: `any` stops at the first true operand and
    /// `all` at the first false one.
    fn condition(&mut self, condition: &ConditionNode) -> Result<(), WorkflowValidationError> {
        match condition {
            ConditionNode::Truthy { left } => {
                self.expression(left)?;
                self.call_intrinsic(TRUTHY_INTRINSIC, 1);
            }
            ConditionNode::Not(inner) => {
                self.condition(inner)?;
                self.call_intrinsic(NOT_INTRINSIC, 1);
            }
            ConditionNode::Compare { left, op, right } => {
                self.expression(left)?;
                self.expression(right)?;
                self.call_intrinsic(compare_intrinsic(*op), 2);
            }
            ConditionNode::Exists { left, expected } => {
                self.expression(left)?;
                self.call_intrinsic(EXISTS_INTRINSIC, 1);
                if !*expected {
                    self.call_intrinsic(NOT_INTRINSIC, 1);
                }
            }
            ConditionNode::All(items) => self.junction(items, false)?,
            ConditionNode::Any(items) => self.junction(items, true)?,
            // an unrecognized shape evaluates to the same error the evaluator raises, so a program
            // carrying one fails where it is used rather than where it is assembled.
            ConditionNode::Other(value) => {
                return Err(WorkflowValidationError::InvalidComputeProgram(format!(
                    "unsupported condition shape: {value}"
                )));
            }
        }
        Ok(())
    }

    /// `all`/`any` as short-circuiting jumps. `stop_on` is the operand value that decides the whole
    /// junction: `true` for `any`, `false` for `all`.
    fn junction(
        &mut self,
        items: &[ConditionNode],
        stop_on: bool,
    ) -> Result<(), WorkflowValidationError> {
        // an empty `all` is true and an empty `any` is false, matching boolean identity.
        if items.is_empty() {
            self.emit(InvocationInstruction::Const {
                value: Value::Bool(!stop_on),
            });
            return Ok(());
        }
        let mut shortcuts = Vec::new();
        let last = items.len() - 1;
        for (index, item) in items.iter().enumerate() {
            self.condition(item)?;
            if index == last {
                break;
            }
            // `any` jumps out when an operand is true, so it tests the operand as-is; `all` jumps
            // out when one is false, which `JumpIfFalse` already tests.
            if stop_on {
                self.call_intrinsic(NOT_INTRINSIC, 1);
            }
            let out = self.emit(InvocationInstruction::JumpIfFalse { target: 0 });
            shortcuts.push(out);
        }
        let over = self.emit(InvocationInstruction::Jump { target: 0 });
        for at in shortcuts {
            self.patch(at);
        }
        self.emit(InvocationInstruction::Const {
            value: Value::Bool(stop_on),
        });
        self.patch(over);
        Ok(())
    }

    fn call_intrinsic(&mut self, name: &str, argc: usize) {
        self.emit(InvocationInstruction::Call {
            target: CallableTarget::Intrinsic {
                name: name.to_string(),
            },
            argc,
            names: Vec::new(),
            policy: None,
        });
    }

    fn expression(
        &mut self,
        expression: &WorkflowExpression,
    ) -> Result<(), WorkflowValidationError> {
        match expression {
            WorkflowExpression::Literal(value) => self.literal(value)?,
            WorkflowExpression::Ref(reference) => self.reference(reference)?,
            WorkflowExpression::Call { name, args } => {
                for arg in args {
                    self.expression(arg)?;
                }
                if crate::compute::is_higher_order(name) {
                    self.emit(InvocationInstruction::HigherOrder {
                        name: name.clone(),
                        argc: args.len(),
                    });
                    return Ok(());
                }
                self.emit(InvocationInstruction::Call {
                    target: self.target_for(name),
                    argc: args.len(),
                    names: Vec::new(),
                    policy: None,
                });
            }
            WorkflowExpression::Apply { callee, args } => {
                self.expression(callee)?;
                for arg in args {
                    self.expression(arg)?;
                }
                self.emit(InvocationInstruction::Apply { argc: args.len() });
            }
            WorkflowExpression::Lambda { params, body } => {
                let mut inner = Assembler::new(self.catalog);
                inner.expression(body)?;
                inner.emit(InvocationInstruction::Return);
                self.emit(InvocationInstruction::Closure {
                    params: params.clone(),
                    body: InvocationProgram::new(inner.instructions),
                });
            }
            // lazy: only the taken branch is assembled into the executed path.
            WorkflowExpression::Cond {
                condition,
                then,
                otherwise,
            } => {
                self.expression(condition)?;
                let to_else = self.emit(InvocationInstruction::JumpIfFalse { target: 0 });
                self.expression(then)?;
                let over_else = self.emit(InvocationInstruction::Jump { target: 0 });
                self.patch(to_else);
                self.expression(otherwise)?;
                self.patch(over_else);
            }
            // lazy: stop at the first non-null operand rather than evaluating the rest.
            WorkflowExpression::Coalesce(items) => self.coalesce(items)?,
            // eager operators. each is an intrinsic call so the vm has one dispatch path, and the
            // library implementation is the same code the evaluator folds with.
            WorkflowExpression::Concat(items) => self.operator(CONCAT_INTRINSIC, items)?,
            WorkflowExpression::Add(items) => self.fold("add", items)?,
            WorkflowExpression::Sub(items) => self.fold("sub", items)?,
            WorkflowExpression::Mul(items) => self.fold("mul", items)?,
            WorkflowExpression::Div(items) => self.fold("div", items)?,
            WorkflowExpression::Mod(items) => self.fold("mod", items)?,
            WorkflowExpression::ToString(inner) => {
                self.operator(TO_STRING_INTRINSIC, std::slice::from_ref(&**inner))?
            }
            WorkflowExpression::ToJsonString(inner) => {
                self.operator(TO_JSON_INTRINSIC, std::slice::from_ref(&**inner))?
            }
            WorkflowExpression::Neg(inner) => {
                self.operator(NEG_INTRINSIC, std::slice::from_ref(&**inner))?
            }
        }
        Ok(())
    }

    /// Literal containers are declarative expression containers, not opaque JSON. Their children
    /// still resolve references and calls, so construct them on the VM stack rather than pushing a
    /// single constant.
    fn literal(&mut self, value: &Value) -> Result<(), WorkflowValidationError> {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.expression(
                        &WorkflowExpression::try_from(item)
                            .map_err(|err| WorkflowValidationError::InvalidValueRef(err.0))?,
                    )?;
                }
                self.emit(InvocationInstruction::Array { len: items.len() });
            }
            Value::Object(map) => {
                for value in map.values() {
                    self.expression(
                        &WorkflowExpression::try_from(value)
                            .map_err(|err| WorkflowValidationError::InvalidValueRef(err.0))?,
                    )?;
                }
                self.emit(InvocationInstruction::Object {
                    keys: map.keys().cloned().collect(),
                });
            }
            _ => {
                self.emit(InvocationInstruction::Const {
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }

    /// fold an n-ary arithmetic operator into left-associated binary calls.
    ///
    /// `a + b + c` is `add(add(a, b), c)`. the library's `add` is binary and the evaluator folds
    /// left-to-right, so emitting one three-argument call would be calling a function that does not
    /// exist — and an n-ary implementation would be a second definition of arithmetic to keep in
    /// step with the first.
    fn fold(
        &mut self,
        name: &str,
        items: &[WorkflowExpression],
    ) -> Result<(), WorkflowValidationError> {
        let Some((first, rest)) = items.split_first() else {
            return Err(WorkflowValidationError::InvalidComputeProgram(format!(
                "'{name}' requires at least one operand"
            )));
        };
        self.expression(first)?;
        for item in rest {
            self.expression(item)?;
            self.call_intrinsic(name, 2);
        }
        Ok(())
    }

    /// compile a `$ref`.
    ///
    /// every root resolves through `LoadRef` against the run context — except `let`, which does not
    /// live there. the VM keeps compute locals in the frame, which is what makes them survive a continuation round trip
    /// without being carried in (and rewritten back out of) the run context on every yield. so a
    /// local reference is rewritten here into the instruction that reads a frame local, and any
    /// path beyond the binding name becomes ordinary indexing.
    fn reference(
        &mut self,
        reference: &runinator_models::workflow_ast::WorkflowValueRef,
    ) -> Result<(), WorkflowValidationError> {
        use runinator_models::workflow_ast::{WorkflowPathSegment, WorkflowRefSource};

        let WorkflowRefSource::Local = reference.source else {
            self.emit(InvocationInstruction::LoadRef {
                reference: crate::expressions::serialize_value_ref(reference),
            });
            return Ok(());
        };
        let Some((WorkflowPathSegment::Key(name), rest)) = reference.path.split_first() else {
            return Err(WorkflowValidationError::InvalidComputeProgram(
                "a 'let' reference must name a binding".into(),
            ));
        };
        self.emit(InvocationInstruction::LoadLocal { name: name.clone() });
        for segment in rest {
            let key = match segment {
                WorkflowPathSegment::Key(key) => Value::from(key.as_str()),
                WorkflowPathSegment::Index(index) => Value::from(*index as i64),
            };
            self.emit(InvocationInstruction::Const { value: key });
            self.call_intrinsic("at", 2);
        }
        Ok(())
    }

    fn operator(
        &mut self,
        name: &str,
        items: &[WorkflowExpression],
    ) -> Result<(), WorkflowValidationError> {
        if items.is_empty() {
            return Err(WorkflowValidationError::InvalidComputeProgram(format!(
                "'{name}' requires at least one operand"
            )));
        }
        for item in items {
            self.expression(item)?;
        }
        self.emit(InvocationInstruction::Call {
            target: CallableTarget::Intrinsic {
                name: name.to_string(),
            },
            argc: items.len(),
            names: Vec::new(),
            policy: None,
        });
        Ok(())
    }

    /// `a ?? b ?? c` as jumps: evaluate `a`, and if it is non-null jump past everything else.
    ///
    /// there is no "jump if not null" instruction, so nullness is tested by an intrinsic and the
    /// candidate is re-loaded on the taken path. the alternative — an eager n-ary `coalesce` call —
    /// would evaluate every operand, which changes the meaning of a recursive right-hand side from
    /// "only if needed" to "always".
    fn coalesce(&mut self, items: &[WorkflowExpression]) -> Result<(), WorkflowValidationError> {
        if items.is_empty() {
            return Err(WorkflowValidationError::InvalidComputeProgram(
                "'$coalesce' requires at least one operand".into(),
            ));
        }
        let mut done = Vec::new();
        let last = items.len() - 1;
        for (index, item) in items.iter().enumerate() {
            self.expression(item)?;
            if index == last {
                break;
            }
            // stash the candidate, test it, and either keep it or drop it and try the next.
            let slot = format!("$coalesce${index}");
            self.emit(InvocationInstruction::StoreLocal { name: slot.clone() });
            self.emit(InvocationInstruction::LoadLocal { name: slot.clone() });
            self.call_intrinsic(EXISTS_INTRINSIC, 1);
            // `JumpIfFalse` on "exists" jumps exactly when the candidate was null, which is when the
            // next operand should be tried. testing `$is_null` here instead would invert it.
            let try_next = self.emit(InvocationInstruction::JumpIfFalse { target: 0 });
            self.emit(InvocationInstruction::LoadLocal { name: slot });
            done.push(self.emit(InvocationInstruction::Jump { target: 0 }));
            self.patch(try_next);
        }
        for at in done {
            self.patch(at);
        }
        Ok(())
    }

    /// classify a called name into what the vm should do with it.
    ///
    /// the catalog is the single source: an intrinsic folds in process, a user function enters a
    /// module frame, and anything else is a provider dispatch. this is the one place the compiler's
    /// idea of "what is callable" meets the vm's.
    fn target_for(&self, name: &str) -> CallableTarget {
        self.catalog.target_for(name)
    }
}

/// operators the surface has but the author-facing library does not name.
pub const CONCAT_INTRINSIC: &str = "$concat";
pub const TO_STRING_INTRINSIC: &str = "$to_string";
pub const TO_JSON_INTRINSIC: &str = "$to_json_string";
pub const NEG_INTRINSIC: &str = "$neg";
pub const IS_NULL_INTRINSIC: &str = "$is_null";
pub const TRUTHY_INTRINSIC: &str = "$truthy";
pub const NOT_INTRINSIC: &str = "$not";
pub const EXISTS_INTRINSIC: &str = "$exists";
pub const IN_INTRINSIC: &str = "$in";

/// every operator intrinsic the assembler can emit that is not in the author-facing catalog.
///
/// they are `$`-prefixed so they cannot collide with a user function or a library name, and so a
/// program that names one directly is obviously machine-generated.
pub const OPERATOR_INTRINSICS: &[&str] = &[
    CONCAT_INTRINSIC,
    TO_STRING_INTRINSIC,
    TO_JSON_INTRINSIC,
    NEG_INTRINSIC,
    IS_NULL_INTRINSIC,
    TRUTHY_INTRINSIC,
    NOT_INTRINSIC,
    EXISTS_INTRINSIC,
    IN_INTRINSIC,
];

/// the comparison intrinsic one declarative operator tests through.
///
/// exhaustive rather than defaulted: a new comparator must be given an implementation here, or a
/// condition using it would silently compile to the wrong test.
fn compare_intrinsic(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Equals => "eq",
        CompareOp::NotEquals => "ne",
        CompareOp::GreaterThan => "gt",
        CompareOp::GreaterThanOrEqual => "gte",
        CompareOp::LessThan => "lt",
        CompareOp::LessThanOrEqual => "lte",
        CompareOp::Contains => "contains",
        // `in` is `contains` with the operands the other way round; the assembler pushes left then
        // right, so the swap has to happen in the intrinsic rather than in the operand order.
        CompareOp::In => IN_INTRINSIC,
        CompareOp::StartsWith => "starts_with",
        CompareOp::EndsWith => "ends_with",
    }
}
