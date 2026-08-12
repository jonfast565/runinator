//! covers the registration retry envelope.

use super::*;

#[test]
fn register_backoff_grows_then_caps() {
    assert_eq!(register_backoff(1), REGISTER_BASE_BACKOFF);
    assert_eq!(register_backoff(2), REGISTER_BASE_BACKOFF * 2);
    assert_eq!(register_backoff(3), REGISTER_BASE_BACKOFF * 4);
    // large attempts saturate at the cap instead of overflowing the shift.
    assert_eq!(register_backoff(64), REGISTER_MAX_BACKOFF);
    assert_eq!(register_backoff(u32::MAX), REGISTER_MAX_BACKOFF);
}
