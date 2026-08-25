//! Applies per-aircraft health after the game initializes a launched aircraft.

use std::{arch::naked_asm, collections::HashMap, slice, sync::OnceLock};

use patchy::{Patch, Result, ReturnType};

use crate::{config::DEFAULT_PLANE_HEALTH, structs::aircraft_body::AircraftBody};

const ORIGINAL_BYTES: [u8; 6] = [0x8b, 0x8e, 0x88, 0x01, 0x00, 0x00];

#[cfg(feature = "1_151")]
const HOOK_ADDRESS: usize = 0x14002f641;
#[cfg(not(feature = "1_151"))]
const HOOK_ADDRESS: usize = 0x14002ee18;

static PLANE_HEALTH: OnceLock<HashMap<String, f32>> = OnceLock::new();

pub(crate) fn install_plane_health(configured_health: HashMap<String, f32>) {
    let mut health_overrides = HashMap::new();

    for (plane_name, health) in configured_health {
        if !is_valid_health(health) {
            log::warn!(
                "plane_health: ignoring invalid health {health} for plane '{plane_name}'; using {DEFAULT_PLANE_HEALTH}"
            );
            continue;
        }

        if health != DEFAULT_PLANE_HEALTH {
            health_overrides.insert(plane_name, health);
        }
    }

    if PLANE_HEALTH.set(health_overrides).is_err() {
        log::error!("plane_health: plane health definitions were already initialized");
    }
}

/// Prepares the aircraft-health hook when at least one custom value is configured.
pub unsafe fn patch_plane_health() -> Result {
    let Some(health_overrides) = PLANE_HEALTH.get() else {
        log::error!("plane_health: plane health definitions were not initialized");
        return Ok(());
    };

    if health_overrides.is_empty() {
        log::info!("No custom aircraft health configured, skipping patch.");
        return Ok(());
    }

    let found_bytes = slice::from_raw_parts(HOOK_ADDRESS as *const u8, ORIGINAL_BYTES.len());
    if found_bytes != ORIGINAL_BYTES {
        log::error!(
            "plane_health: unexpected bytes at {HOOK_ADDRESS:#x}; expected {:02x?}, found {:02x?}",
            ORIGINAL_BYTES,
            found_bytes
        );
        return Ok(());
    }

    Patch::patch_call(
        HOOK_ADDRESS,
        apply_plane_health_bridge as *const (),
        ORIGINAL_BYTES.len(),
        true,
        ReturnType::None,
    )?;

    log::info!("Prepared custom aircraft-health hook at {HOOK_ADDRESS:#x}");
    Ok(())
}

/// At the hook site RSI contains the aircraft body. The overwritten instruction is
/// replayed before this bridge runs, and patchy restores its ECX result afterward.
#[unsafe(naked)]
unsafe extern "C" fn apply_plane_health_bridge() {
    naked_asm!(
        "mov rcx, rsi",
        "jmp {helper}",
        helper = sym apply_plane_health,
    );
}

unsafe extern "C" fn apply_plane_health(aircraft: *mut AircraftBody) {
    if aircraft.is_null() {
        return;
    }

    let aircraft = &mut *aircraft;
    let Some(&health) = PLANE_HEALTH
        .get()
        .and_then(|planes| planes.get(aircraft.plane_name.get_string()))
    else {
        return;
    };

    aircraft.health = health;
    aircraft.max_health = health;

    log::debug!(
        "plane_health: configured plane '{}' with {health} health",
        aircraft.plane_name.get_string()
    );
}

fn is_valid_health(health: f32) -> bool {
    health.is_finite() && health > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_positive_finite_health() {
        assert!(is_valid_health(1.0));
        assert!(is_valid_health(DEFAULT_PLANE_HEALTH));
    }

    #[test]
    fn rejects_non_positive_or_non_finite_health() {
        assert!(!is_valid_health(0.0));
        assert!(!is_valid_health(-1.0));
        assert!(!is_valid_health(f32::INFINITY));
        assert!(!is_valid_health(f32::NAN));
    }
}
