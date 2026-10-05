//! Helpers shared by the property-test files.

/// Proptest case count: the `PROPTEST_CASES` environment variable if it parses, else `default`.
/// (An explicit `ProptestConfig::with_cases` would otherwise override proptest's own reading of
/// that variable, so nightly runs could not scale the case count up.)
pub fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}
