use highfleet::general::EscadraString;

#[cfg(feature = "1_151")]
const HEALTH_OFFSET: usize = 0x2b8;
#[cfg(not(feature = "1_151"))]
const HEALTH_OFFSET: usize = 0x2c8;

/// Partial layout of the game-owned aircraft body shared by supported versions.
///
/// Only fields needed by aircraft configuration hooks are exposed. Unknown regions
/// preserve the verified native offsets without assigning speculative meanings.
#[repr(C, align(8))]
pub struct AircraftBody {
    _unknown_000: [u8; 0x190],

    /// Internal aircraft name used as the key in the plane configuration.
    pub plane_name: EscadraString,

    _unknown_1b0: [u8; HEALTH_OFFSET - 0x1b0],

    /// Current aircraft health.
    pub health: f32,
    /// Maximum aircraft health.
    pub max_health: f32,
}

const _: () = {
    assert!(std::mem::offset_of!(AircraftBody, plane_name) == 0x190);
    assert!(std::mem::offset_of!(AircraftBody, health) == HEALTH_OFFSET);
    assert!(std::mem::offset_of!(AircraftBody, max_health) == HEALTH_OFFSET + 4);
    assert!(std::mem::size_of::<AircraftBody>() == HEALTH_OFFSET + 8);
    assert!(std::mem::align_of::<AircraftBody>() == 8);
};
