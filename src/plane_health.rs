//! Applies per-aircraft health after the game initializes a launched aircraft.

use std::{collections::HashMap, sync::OnceLock};

#[cfg(feature = "1_151")]
use highfleet::v1_151::Body;
#[cfg(not(feature = "1_151"))]
use highfleet::v1_163::Body;
use patchy::{CallBridge, FlagPolicy, KeepReturn, PatchSession, Result, StackAlignment};

use crate::{config::DEFAULT_PLANE_HEALTH, replay_call::replay_then_call};

/// `MOV ECX, [RSI + 0x188]`, replayed before the hook so its ECX result survives.
const HOOK_SIZE: usize = 6;
/// `MOV RCX, RSI`: the aircraft's ordinary Body.
const PASS_AIRCRAFT: [u8; 3] = [0x48, 0x89, 0xf1];

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
pub unsafe fn patch_plane_health(session: &mut PatchSession) -> Result {
    let Some(health_overrides) = PLANE_HEALTH.get() else {
        log::error!("plane_health: plane health definitions were not initialized");
        return Ok(());
    };

    if health_overrides.is_empty() {
        log::info!("No custom aircraft health configured, skipping patch.");
        return Ok(());
    }

    replay_then_call(
        session,
        HOOK_ADDRESS,
        HOOK_SIZE,
        CallBridge {
            callback: apply_plane_health as *const (),
            incoming_stack: StackAlignment::Aligned16,
            flags: FlagPolicy::Preserve,
            keep_return: KeepReturn::Neither,
            before_call: &PASS_AIRCRAFT,
            after_call: &[],
        },
    )?;

    log::info!("Prepared custom aircraft-health hook at {HOOK_ADDRESS:#x}");
    Ok(())
}

unsafe extern "C" fn apply_plane_health(aircraft: *mut Body) {
    if aircraft.is_null() {
        return;
    }

    let aircraft = &mut *aircraft;
    let Some(&health) = PLANE_HEALTH
        .get()
        .and_then(|planes| planes.get(aircraft.oid.get_string()))
    else {
        return;
    };

    aircraft.health = health;
    aircraft.health_max = health;

    log::debug!(
        "plane_health: configured plane '{}' with {health} health",
        aircraft.oid.get_string()
    );
}

fn is_valid_health(health: f32) -> bool {
    health.is_finite() && health > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_body_fields_match_aircraft_health_hook_offsets() {
        use std::mem::{align_of, offset_of};

        #[cfg(feature = "1_151")]
        let health_offset = 0x2b8;
        #[cfg(not(feature = "1_151"))]
        let health_offset = 0x2c8;

        // The native aircraft identifier is Body's serialized m_oid, not Node's m_name.
        assert_eq!(offset_of!(Body, oid), 0x190);
        assert_eq!(offset_of!(Body, health), health_offset);
        assert_eq!(offset_of!(Body, health_max), health_offset + 4);
        assert_eq!(align_of::<Body>(), 8);
    }

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
