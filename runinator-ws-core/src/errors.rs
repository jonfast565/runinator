use runinator_models::errors::ErrorDescriptor;

// execution-profile bundle retrieval, in the web-service family's range.
//
// One handler answered nine distinct causes with the same sentence, and the response body has to
// stay that vague: the caller is a worker or a desktop agent that has not proved it may know
// whether the profile exists at all. The cost was that an operator debugging a worker that could
// not materialize a profile had nothing to go on either. These name the rule that refused, for the
// log and the audit trail, while the body the caller sees is unchanged.

pub const PROFILE_ROLE_REFUSED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI191",
    "ws.execution_profile.role_refused",
    "Caller does not hold a system role that may fetch an execution profile bundle",
);
pub const PROFILE_NOT_ADMITTED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI192",
    "ws.execution_profile.not_admitted",
    "Caller is not an admitted consumer of this execution profile",
);
pub const PROFILE_NOT_VISIBLE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI193",
    "ws.execution_profile.not_visible",
    "Execution profile is not visible to this tenant, is disabled, or is not at the requested revision",
);
pub const PROFILE_EXPIRED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI194",
    "ws.execution_profile.expired",
    "Execution profile has expired",
);
pub const PROFILE_REVISION_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI195",
    "ws.execution_profile.revision_missing",
    "Execution profile revision has no stored bundle",
);
pub const PROFILE_STORAGE_UNREADABLE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI196",
    "ws.execution_profile.storage_unreadable",
    "Execution profile bundle could not be read from object storage",
);
pub const PROFILE_UNDECRYPTABLE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI197",
    "ws.execution_profile.undecryptable",
    "Execution profile bundle could not be decrypted",
);
pub const PROFILE_INTEGRITY: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI198",
    "ws.execution_profile.integrity",
    "Execution profile bundle failed its integrity check",
);
pub const PROFILE_STORAGE_URI: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI199",
    "ws.execution_profile.storage_uri",
    "Execution profile revision has an unusable storage URI",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    PROFILE_ROLE_REFUSED,
    PROFILE_NOT_ADMITTED,
    PROFILE_NOT_VISIBLE,
    PROFILE_EXPIRED,
    PROFILE_REVISION_MISSING,
    PROFILE_STORAGE_UNREADABLE,
    PROFILE_UNDECRYPTABLE,
    PROFILE_INTEGRITY,
    PROFILE_STORAGE_URI,
];
