//! Static semantic keys used by first-version shell and game surfaces.

use crate::semantic::SemanticKey;

/// Constructs a static surface key. These literals are reviewed vocabulary.
#[must_use]
pub fn surface_key(value: &'static str) -> SemanticKey {
    match SemanticKey::new(value) {
        Ok(key) => key,
        Err(error) => unreachable!("static surface key `{value}` is valid: {error}"),
    }
}

/// Constructs a formatted surface key.
///
/// # Errors
///
/// Returns the semantic-key error when `value` is not a stable path.
pub fn try_surface_key(value: impl Into<String>) -> Result<SemanticKey, crate::SemanticKeyError> {
    SemanticKey::new(value)
}
