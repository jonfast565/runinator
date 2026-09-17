#[allow(unused_imports)]
use super::*;

/// one resolved callable.
#[derive(Debug, Clone)]
pub struct CallableEntry {
    /// the name a program calls this under.
    pub name: String,
    pub kind: CallableKind,
    /// how far this call's result can travel from where it is computed.
    pub effect: EffectClass,
    /// the typed signature, when one is known.
    pub signature: Option<ActionMetadata>,
    /// accepted argument count `(min, max)`, when known.
    pub arity: Option<(usize, usize)>,
}

impl CallableEntry {
    /// the invocation-ir target this entry compiles to.
    pub fn target(&self) -> CallableTarget {
        match &self.kind {
            CallableKind::Intrinsic | CallableKind::HigherOrder => CallableTarget::Intrinsic {
                name: self.name.clone(),
            },
            CallableKind::Local => CallableTarget::Local {
                name: self.name.clone(),
            },
            CallableKind::Provider { provider, function } => CallableTarget::Provider {
                provider: provider.clone(),
                function: function.clone(),
            },
            CallableKind::Packaged { binding } => CallableTarget::Packaged {
                binding: (**binding).clone(),
            },
        }
    }

    /// whether a call to this can be completed inside the reducer.
    pub fn is_in_process(&self) -> bool {
        self.effect.is_in_process()
    }

    /// whether the argument count is acceptable, when the arity is known.
    pub fn accepts_argc(&self, argc: usize) -> bool {
        match self.arity {
            Some((min, max)) => argc >= min && argc <= max,
            None => true,
        }
    }
}
