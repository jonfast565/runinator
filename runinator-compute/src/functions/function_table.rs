#[allow(unused_imports)]
use super::*;

/// the user functions a workflow carries, keyed by name. parsed from `metadata.functions`.
#[derive(Default)]
pub struct FunctionTable {
    pub(super) functions: HashMap<String, RuntimeFunction>,
}

impl FunctionTable {
    /// look up a function by name.
    pub fn get(&self, name: &str) -> Option<&RuntimeFunction> {
        self.functions.get(name)
    }

    /// whether the table carries no functions.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// parse the `metadata.functions` array into a runtime table. `None` (no functions section)
    /// yields an empty table. each entry is `{ name, params: [{name,...}|"name"], body, recursive? }`.
    pub fn from_metadata(value: Option<&Value>) -> Result<Self, WorkflowValidationError> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        // A json `null` is the wire sentinel for "no functions section", so treat it the same as
        // an absent value.
        if value.is_null() {
            return Ok(Self::default());
        }
        let items = value.as_array().ok_or_else(|| {
            WorkflowValidationError::InvalidValueRef("metadata.functions must be an array".into())
        })?;
        let mut functions = HashMap::with_capacity(items.len());
        for item in items {
            let function = parse_function(item)?;
            let object = item.as_object();
            let name = object
                .and_then(|map| map.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    WorkflowValidationError::InvalidValueRef("function requires a name".into())
                })?;
            functions.insert(name.to_string(), function);
        }
        Ok(Self { functions })
    }
}

impl FunctionTable {
    pub(crate) fn catalog(&self) -> CallableCatalog {
        let mut catalog = CallableCatalog::builtin();
        for (name, function) in &self.functions {
            catalog.add_local(
                name.clone(),
                function.params.len(),
                runinator_models::invocation::EffectClass::Pure,
            );
        }
        catalog
    }

    /// Assemble a declarative expression and every user function into one VM module. Local calls
    /// enter hermetic named-function frames; the entry expression keeps its normal run context.
    pub(crate) fn module_for_expression(
        &self,
        expression: &WorkflowExpression,
    ) -> Result<InvocationModule, WorkflowValidationError> {
        let functions =
            self.functions
                .iter()
                .map(|(name, function)| {
                    let body = match &function.body {
                        FunctionBody::Expr(expression) => ComputeProgram(vec![
                            runinator_models::workflow_ast::ComputeStmt::Return(expression.clone()),
                        ]),
                        FunctionBody::Program(program) => program.clone(),
                    };
                    (
                        name.clone(),
                        function.params.clone(),
                        body,
                        function.max_depth,
                    )
                })
                .collect::<Vec<_>>();
        let module = assemble_module(&ComputeProgram::default(), &functions, &self.catalog())?;
        Ok(InvocationModule {
            entry: assemble_expression(expression, &self.catalog())?,
            ..module
        })
    }
}
