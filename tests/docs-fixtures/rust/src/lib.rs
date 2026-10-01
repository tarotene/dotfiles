//! Fixture crate for the docs-rust composite action.

/// Adds two numbers.
///
/// ```
/// assert_eq!(docs_fixture::add(1, 2), 3);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
