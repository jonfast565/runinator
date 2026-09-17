#[allow(unused_imports)]
use super::*;

/// An immutable compiled workflow snapshot attached to a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowModule {
    pub version: u32,
    pub instructions: Vec<WorkflowInstruction>,
    /// Maps executable locations back to the author-facing graph.
    #[serde(default)]
    pub source_map: Vec<WorkflowSourceMapEntry>,
    /// Compiled interrupt handler entries. Frozen into the module rather than looked up in mutable
    /// workflow metadata, so a run in flight keeps the handlers it started with. Timer handlers
    /// are keyed by their distinct `timer_id`, allowing several periodic handlers in one workflow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interrupt_handlers: Vec<WorkflowVmInterruptHandler>,
}

impl WorkflowModule {
    pub fn new(instructions: Vec<WorkflowInstruction>) -> Self {
        Self {
            version: WORKFLOW_VM_VERSION,
            instructions,
            source_map: Vec::new(),
            interrupt_handlers: Vec::new(),
        }
    }

    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_VM_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_vm_version(
            WorkflowVmRecordKind::Module,
            WORKFLOW_VM_VERSION,
            self.version,
        )?;
        for entry in &self.source_map {
            entry.ensure_supported()?;
        }
        Ok(())
    }

    /// Return the graph location containing an instruction pointer.
    ///
    /// The compiler lays blocks out consecutively, so the ranges are sorted and disjoint and this
    /// can bisect rather than scan — it is called once per drive and once per rendered cursor.
    /// `source_map_is_ordered` pins the invariant this relies on.
    pub fn graph_location(&self, ip: usize) -> Option<&WorkflowSourceMapEntry> {
        let index = self
            .source_map
            .binary_search_by(|entry| {
                if entry.instruction_end <= ip {
                    std::cmp::Ordering::Less
                } else if entry.instruction_start > ip {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .ok()?;
        self.source_map.get(index)
    }

    /// Whether the source map is sorted and non-overlapping, which is what makes
    /// [`Self::graph_location`] a bisection. Compiled modules always satisfy it.
    pub fn source_map_is_ordered(&self) -> bool {
        self.source_map.windows(2).all(|pair| {
            pair[0].instruction_start <= pair[0].instruction_end
                && pair[0].instruction_end <= pair[1].instruction_start
        })
    }

    /// The compiled handler for one non-timer source, if the workflow declared it.
    pub fn interrupt_handler(
        &self,
        source: InterruptSource,
    ) -> Option<&WorkflowVmInterruptHandler> {
        self.interrupt_handlers
            .iter()
            .find(|handler| handler.source == source)
    }

    /// Select the frozen handler for a pending request. Timer requests carry the declaration's
    /// stable handler id in their payload, while the other sources remain one-per-source.
    pub fn interrupt_handler_for(
        &self,
        source: InterruptSource,
        payload: &Value,
    ) -> Option<&WorkflowVmInterruptHandler> {
        if source != InterruptSource::Timer {
            return self.interrupt_handler(source);
        }
        let timer_id = payload.get("timer_id").and_then(Value::as_str)?;
        self.interrupt_handlers.iter().find(|handler| {
            handler.source == InterruptSource::Timer
                && handler.timer_id.as_deref() == Some(timer_id)
        })
    }
}
