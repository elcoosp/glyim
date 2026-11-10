/// `ALIGN_MAX`.
pub const ALIGN_MAX: u64 = 64;
/// `ALIGN_MIN`.
pub const ALIGN_MIN: u64 = 1;

/// `validate_alignment`.
///
/// # Errors
///
/// Returns the rejection reason when `align` is zero, not a power of
/// two, or above [`ALIGN_MAX`].
pub fn validate_alignment(align: u64) -> Result<(), String> {
    if align == 0 {
        return Err("alignment must be non-zero".into());
    }
    if !align.is_power_of_two() {
        return Err(format!("alignment {align} is not a power of two"));
    }
    if align > ALIGN_MAX {
        return Err(format!("alignment {align} exceeds ALIGN_MAX ({ALIGN_MAX})"));
    }
    Ok(())
}
