use runinator_models::errors::ErrorDescriptor;

// numbered middleware configuration errors in the web-service family's range.
pub const RATE_LIMIT_RPS: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI188",
    "ws.rate_limit.rps",
    "Invalid rate-limit requests per second",
);
pub const RATE_LIMIT_BURST: ErrorDescriptor =
    ErrorDescriptor::new("RUNI189", "ws.rate_limit.burst", "Invalid rate-limit burst");
pub const RATE_LIMIT_QUOTA: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI190",
    "ws.rate_limit.quota",
    "Unsupported rate-limit refill duration",
);
