use super::*;

#[test]
fn bucket_allows_burst_then_blocks() {
    let limiter = RateLimiter::new(RateLimitConfig {
        enabled: true,
        requests_per_second: 1.0,
        burst: 3.0,
    });
    // three immediate requests fit the burst.
    assert!(limiter.check("k").is_ok());
    assert!(limiter.check("k").is_ok());
    assert!(limiter.check("k").is_ok());
    // the fourth is rejected with a positive retry-after.
    let retry = limiter.check("k").unwrap_err();
    assert!(retry > 0.0);
}

#[test]
fn keys_are_isolated() {
    let limiter = RateLimiter::new(RateLimitConfig {
        enabled: true,
        requests_per_second: 1.0,
        burst: 1.0,
    });
    assert!(limiter.check("a").is_ok());
    // a separate key has its own bucket.
    assert!(limiter.check("b").is_ok());
    assert!(limiter.check("a").is_err());
}
