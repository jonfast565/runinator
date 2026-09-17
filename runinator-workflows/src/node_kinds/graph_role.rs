#[allow(unused_imports)]
use super::*;

/// how the graph walkers treat a node kind.
///
/// each field replaced a free-standing `matches!` list in a different file. a new kind picks its
/// role once here rather than being silently omitted from four separate lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphRole {
    /// may be entered as a branch or body target — true for everything but `start`/`end`/`fail`.
    pub runnable_entry: bool,
    /// an entry point: the runtime places a cursor here, and no edge may target it.
    ///
    /// distinct from `runnable_entry`, which the two flags used to conflate because `start` was the
    /// only entry point and it is not a legal region entry. `interrupt` is both — a handler region
    /// legitimately starts there — so "a cursor may sit here" and "an edge may point here" had to
    /// come apart. [`TargetRule`] reads this one.
    pub entry_point: bool,
    /// settles the run when reached.
    pub terminal: bool,
    /// records an output addressable downstream as `steps.<id>.output`.
    pub produces_output: bool,
    /// re-entered by design, so a back edge to it is a loop rather than a cycle error.
    pub reentrant: bool,
    /// modelled by the dry-run simulator; the rest need fan-out bookkeeping the walk lacks.
    pub simulatable: bool,
    /// may appear inside an interrupt handler region.
    ///
    /// this is an opt-in allowlist, defaulting to `false` on every role: a kind is unsupported in a
    /// handler until someone deliberately supports it. that is what keeps the blast radius of the
    /// feature small — a handler is a bounded side-channel, so it may not park (which would pin the
    /// suspended thread open), fan out (whose cursors have no handler to belong to), or run away.
    pub handler_safe: bool,
    /// a cursor sitting on this kind may be interrupted.
    ///
    /// false where a cursor is not a thread doing work: the graph endpoints, and `join`, where the
    /// cursor represents coordination state rather than a position to come back to.
    pub interruptible: bool,
}

impl GraphRole {
    /// the ordinary case: a runnable step that records an output and the simulator models.
    pub const STEP: Self = Self {
        runnable_entry: true,
        entry_point: false,
        terminal: false,
        produces_output: true,
        reentrant: false,
        simulatable: true,
        handler_safe: false,
        interruptible: true,
    };

    /// `start`: entered only as the run's entry point, and produces nothing addressable.
    pub const START: Self = Self {
        runnable_entry: false,
        entry_point: true,
        terminal: false,
        produces_output: false,
        reentrant: false,
        simulatable: true,
        handler_safe: false,
        interruptible: false,
    };

    /// `end`/`fail`: settles the run, and produces nothing addressable.
    pub const TERMINAL: Self = Self {
        runnable_entry: false,
        entry_point: false,
        terminal: true,
        produces_output: false,
        reentrant: false,
        simulatable: true,
        handler_safe: false,
        interruptible: false,
    };

    /// a step whose output is not addressable downstream.
    pub const fn without_output(self) -> Self {
        Self {
            produces_output: false,
            ..self
        }
    }

    /// a step the dry-run simulator does not model.
    pub const fn not_simulatable(self) -> Self {
        Self {
            simulatable: false,
            ..self
        }
    }

    /// a step a back edge may legitimately return to.
    pub const fn reentrant(self) -> Self {
        Self {
            reentrant: true,
            ..self
        }
    }

    /// a step an interrupt handler region may contain. opt in only for kinds that cannot park, fan
    /// out, or run unbounded — a handler must finish and hand control back.
    pub const fn handler_safe(self) -> Self {
        Self {
            handler_safe: true,
            ..self
        }
    }

    /// a step a cursor may not be interrupted while sitting on.
    pub const fn not_interruptible(self) -> Self {
        Self {
            interruptible: false,
            ..self
        }
    }
}
