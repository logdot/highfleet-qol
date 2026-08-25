//! Overrides HighFleet's fixed aircraft-gun ammo after the native configurator runs.

use std::{arch::naked_asm, collections::HashMap, mem::size_of, slice, sync::OnceLock};

use highfleet::general::EscadraString;

#[cfg(feature = "1_151")]
use highfleet::v1_151::Ammo;
#[cfg(not(feature = "1_151"))]
use highfleet::v1_163::Ammo;
use patchy::{Patch, ReturnType};

use crate::structs::aircraft_logic::AircraftLogic;

const BUILTIN_GUN_AMMO: &str = "ITEM_GUN37";
const MAX_AMMO_DEFINITIONS: usize = 10_000;

#[cfg(feature = "1_151")]
const CONFIGURE_PLANE_GUN_ADDRESS: usize = 0x14000ec20;
#[cfg(feature = "1_151")]
const AMMO_LIST_BEGIN_ADDRESS: usize = 0x1439426e0;
#[cfg(feature = "1_151")]
const AIRCRAFT_LOADOUT_ID_OFFSET: usize = 0x3e8;
#[cfg(feature = "1_151")]
const TOTAL_GUN_AMMO: i32 = 100;
#[cfg(feature = "1_151")]
const EXPECTED_AMMO_SIZE: usize = 0x168;

#[cfg(not(feature = "1_151"))]
const CONFIGURE_PLANE_GUN_ADDRESS: usize = 0x14000ea20;
#[cfg(not(feature = "1_151"))]
const AMMO_LIST_BEGIN_ADDRESS: usize = 0x143a13be0;
#[cfg(not(feature = "1_151"))]
const AIRCRAFT_LOADOUT_ID_OFFSET: usize = 0x3f8;
#[cfg(not(feature = "1_151"))]
const TOTAL_GUN_AMMO: i32 = 50;
#[cfg(not(feature = "1_151"))]
const EXPECTED_AMMO_SIZE: usize = 0x188;

const _: () = assert!(size_of::<Ammo>() == EXPECTED_AMMO_SIZE);

#[derive(Clone, Copy)]
struct CallSite {
    address: usize,
    expected_bytes: [u8; 5],
}

#[cfg(feature = "1_151")]
const CALL_SITES: &[CallSite] = &[CallSite {
    address: 0x14002f502,
    expected_bytes: [0xe8, 0x19, 0xf7, 0xfd, 0xff],
}];

#[cfg(not(feature = "1_151"))]
const CALL_SITES: &[CallSite] = &[
    CallSite {
        address: 0x14002ec99,
        expected_bytes: [0xe8, 0x82, 0xfd, 0xfd, 0xff],
    },
    CallSite {
        address: 0x14002edfb,
        expected_bytes: [0xe8, 0x20, 0xfc, 0xfd, 0xff],
    },
];

static LOADOUT_GUNS: OnceLock<HashMap<String, String>> = OnceLock::new();

pub(crate) fn install_loadout_guns(loadout_guns: HashMap<String, String>) {
    if LOADOUT_GUNS.set(loadout_guns).is_err() {
        log::error!("plane_guns: loadout gun definitions were already initialized");
    }
}

/// Prepares hooks for the game's hardcoded built-in aircraft-gun calls.
pub unsafe fn patch_plane_guns() {
    let Some(loadout_guns) = LOADOUT_GUNS.get() else {
        log::error!("plane_guns: loadout gun definitions were not initialized");
        return;
    };

    if !loadout_guns
        .values()
        .any(|ammo_name| ammo_name != BUILTIN_GUN_AMMO)
    {
        log::info!("No custom aircraft gun ammo configured, skipping patch.");
        return;
    }

    let mut valid = true;
    for call_site in CALL_SITES {
        let found_bytes = slice::from_raw_parts(
            call_site.address as *const u8,
            call_site.expected_bytes.len(),
        );
        if found_bytes != call_site.expected_bytes {
            log::error!(
                "plane_guns: unexpected bytes at {:#x}; expected {:02x?}, found {:02x?}",
                call_site.address,
                call_site.expected_bytes,
                found_bytes
            );
            valid = false;
        }
    }

    if !valid {
        log::error!("plane_guns: no hooks were prepared");
        return;
    }

    for call_site in CALL_SITES {
        let patch = Patch::patch_call(
            call_site.address,
            configure_plane_gun_bridge as *const (),
            call_site.expected_bytes.len(),
            false,
            ReturnType::None,
        );
        std::mem::forget(patch);
    }

    log::info!("Prepared {} custom aircraft-gun hook(s)", CALL_SITES.len());
}

/// At each patched call site RSI contains the aircraft Body, while RCX and RDX
/// contain the original configure-plane-gun arguments. The AircraftLogic back-link
/// is not initialized until later in the launch function, so the Body must be
/// forwarded from this live register instead.
#[unsafe(naked)]
unsafe extern "C" fn configure_plane_gun_bridge() {
    naked_asm!(
        "mov r8, rsi",
        "jmp {helper}",
        helper = sym configure_plane_gun_and_override,
    );
}

unsafe extern "C" fn configure_plane_gun_and_override(
    aircraft_logic: *mut AircraftLogic,
    ammo_name: *mut EscadraString,
    aircraft: *const u8,
) {
    type ConfigurePlaneGunFn = unsafe extern "C" fn(*mut AircraftLogic, *mut EscadraString);

    let configure_plane_gun: ConfigurePlaneGunFn = std::mem::transmute(CONFIGURE_PLANE_GUN_ADDRESS);
    configure_plane_gun(aircraft_logic, ammo_name);

    if aircraft_logic.is_null() || aircraft.is_null() {
        return;
    }

    let Some(loadout_guns) = LOADOUT_GUNS.get() else {
        return;
    };

    let loadout_id = &*(aircraft.add(AIRCRAFT_LOADOUT_ID_OFFSET) as *const EscadraString);
    let loadout_name = loadout_id.get_string();
    let Some(custom_ammo_name) = loadout_guns.get(loadout_name) else {
        return;
    };

    if custom_ammo_name == BUILTIN_GUN_AMMO {
        return;
    }

    let Some(ammo) = find_ammo_definition(custom_ammo_name) else {
        log::warn!(
            "plane_guns: ammo definition '{}' for loadout '{}' was not found; using {}",
            custom_ammo_name,
            loadout_name,
            BUILTIN_GUN_AMMO
        );
        return;
    };

    if ammo.max_load <= 0 {
        log::warn!(
            "plane_guns: ammo definition '{}' for loadout '{}' has invalid gun capacity {}; using {}",
            custom_ammo_name,
            loadout_name,
            ammo.max_load,
            BUILTIN_GUN_AMMO
        );
        return;
    }

    let aircraft_logic = &mut *aircraft_logic;
    aircraft_logic.gun_ammo_index = ammo.index;
    aircraft_logic.gun_rate = ammo.gun_rate;
    aircraft_logic.gun_max_load = ammo.max_load;
    aircraft_logic.gun_load = ammo.max_load;
    aircraft_logic.gun_reserve_ammo = (TOTAL_GUN_AMMO - ammo.max_load).max(0);

    log::debug!(
        "plane_guns: configured loadout '{}' with ammo '{}'",
        loadout_name,
        custom_ammo_name
    );
}

struct FoundAmmo {
    index: i32,
    gun_rate: f32,
    max_load: i32,
}

unsafe fn find_ammo_definition(ammo_name: &str) -> Option<FoundAmmo> {
    let begin = (AMMO_LIST_BEGIN_ADDRESS as *const *const Ammo).read();
    let end = ((AMMO_LIST_BEGIN_ADDRESS + size_of::<*const Ammo>()) as *const *const Ammo).read();

    if begin.is_null() || end.is_null() {
        log::error!("plane_guns: live ammo-definition vector is unavailable");
        return None;
    }

    let begin_address = begin as usize;
    let end_address = end as usize;
    if end_address < begin_address {
        log::error!("plane_guns: live ammo-definition vector has an invalid pointer range");
        return None;
    }

    let byte_length = end_address - begin_address;
    if !byte_length.is_multiple_of(size_of::<Ammo>()) {
        log::error!("plane_guns: live ammo-definition vector has an invalid byte length");
        return None;
    }

    let definition_count = byte_length / size_of::<Ammo>();
    if definition_count > MAX_AMMO_DEFINITIONS {
        log::error!(
            "plane_guns: live ammo-definition vector contains an implausible number of records ({definition_count})"
        );
        return None;
    }

    for index in 0..definition_count {
        let definition = &*begin.add(index);
        if definition.item_name.get_string() == ammo_name {
            let Ok(index) = i32::try_from(index) else {
                return None;
            };
            return Some(FoundAmmo {
                index,
                gun_rate: ammo_gun_rate(definition),
                max_load: ammo_max_load(definition),
            });
        }
    }

    None
}

#[cfg(feature = "1_151")]
fn ammo_gun_rate(ammo: &Ammo) -> f32 {
    ammo.unknown_158h
}

#[cfg(not(feature = "1_151"))]
fn ammo_gun_rate(ammo: &Ammo) -> f32 {
    ammo.fire_delay
}

#[cfg(feature = "1_151")]
fn ammo_max_load(ammo: &Ammo) -> i32 {
    ammo.unknown_15ch
}

#[cfg(not(feature = "1_151"))]
fn ammo_max_load(ammo: &Ammo) -> i32 {
    ammo.unknown_180h
}
