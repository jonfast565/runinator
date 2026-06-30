use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// stable error dictionary for the provisioner crate. PROVISION prefix per the engine catalog rules.
pub const BACKEND_UNAVAILABLE: ErrorDescriptor = ErrorDescriptor::new(
    "PROVISION001",
    "provision.backend",
    "Provisioning backend unavailable",
);
pub const UNKNOWN_BACKEND: ErrorDescriptor = ErrorDescriptor::new(
    "PROVISION002",
    "provision.unknown",
    "Unknown provisioning backend",
);
pub const UNSUPPORTED_KIND: ErrorDescriptor = ErrorDescriptor::new(
    "PROVISION003",
    "provision.kind",
    "Backend cannot provision this node kind",
);
pub const SNAPSHOT_READ: ErrorDescriptor = ErrorDescriptor::new(
    "PROVISION004",
    "provision.snapshot",
    "Could not read supervisor state snapshot",
);
pub const ENQUEUE_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "PROVISION005",
    "provision.enqueue",
    "Could not enqueue supervisor control command",
);
pub const KUBERNETES_API: ErrorDescriptor = ErrorDescriptor::new(
    "PROVISION006",
    "provision.k8s.api",
    "Kubernetes API request failed",
);
pub const KUBERNETES_INIT: ErrorDescriptor = ErrorDescriptor::new(
    "PROVISION007",
    "provision.k8s.init",
    "Could not initialize Kubernetes client",
);

const DICTIONARY: &[ErrorDescriptor] = &[
    BACKEND_UNAVAILABLE,
    UNKNOWN_BACKEND,
    UNSUPPORTED_KIND,
    SNAPSHOT_READ,
    ENQUEUE_FAILED,
    KUBERNETES_API,
    KUBERNETES_INIT,
];

/// engine error dictionary for the provisioner crate.
pub struct ProvisionerErrors;

impl EngineErrors for ProvisionerErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
