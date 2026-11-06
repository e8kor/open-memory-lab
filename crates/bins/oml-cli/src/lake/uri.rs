//! A lake's canonical location, the anchor's `uri` (ADR-0062 d5).

/// # Errors
/// Why a location is not one an anchor may carry.
pub fn canonical(_location: &str) -> Result<String, String> {
    Err("not built".into())
}
