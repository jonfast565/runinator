#[allow(unused_imports)]
use super::*;

/// one declared handler: which source it answers, the region it enters, and whether it may fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterruptDeclaration {
    /// the source this handler answers. stored as a string so an unknown source from a newer
    /// binary is ignored rather than failing the whole definition parse.
    pub on: String,
    /// the region's entry node id.
    pub handler: String,
    /// whether this link may raise its handler. absent on older definitions means enabled.
    #[serde(default = "interrupt_enabled", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Cadence for a `timer` interrupt. Kept on the declaration rather than the handler node: a
    /// timer is a run-level source, while a handler region remains pure graph structure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<i64>,
}

impl InterruptDeclaration {
    /// the parsed source, or `None` when this declaration names a source this binary does not know.
    pub fn source(&self) -> Option<InterruptSource> {
        self.on.parse().ok()
    }
}
