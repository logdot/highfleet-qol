//! Patches the SomethingWithLevelSerias function to inject custom parts into generated shops.
//!
//! This module hooks after an existing DefinePart call in the weapons category section
//! of the shop generation function, and calls DefinePart for each user-configured part string.
//!
//! Each part has a configurable probability of appearing and a random count in
//! `[min_parts, max_parts]`, rolled fresh every time the shop generation runs.

use std::{collections::HashMap, ffi::CString};

use crate::{
    config::ShopPart,
    patchy::{Patch, ReturnType},
    rng,
};

/// Stored representation of a custom part with its RNG parameters.
struct CustomPart {
    /// Null-terminated model ID string for DefinePart.
    moid: CString,
    /// Probability in [0.0, 1.0] that this part appears in a shop.
    probability: f32,
    /// Minimum number of this part to spawn (inclusive).
    min_parts: u32,
    /// Maximum number of this part to spawn (inclusive).
    max_parts: u32,
    /// Optional list of city types (1–7) where this part can appear.
    /// If empty, the part appears in all city types.
    city_types: Vec<u32>,
}

/// The list of custom parts (with config) to inject into weapon shops.
static mut CUSTOM_PARTS: Vec<CustomPart> = Vec::new();

// DefinePart function address
// Body * __fastcall DefinePart(Body * allPartLibrary, char * moid, Node * categoryLibrary, int count)
// On x86_64 Windows, __fastcall is the standard calling convention (RCX, RDX, R8, R9).
#[cfg(feature = "1_151")]
const DEFINE_PART_FN: usize = 0x1401fde40;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const DEFINE_PART_FN: usize = 0x14021b190;

// Address of pointer to allPartLibrary (first arg to DefinePart, in RCX)
#[cfg(feature = "1_151")]
const ALL_PART_LIBRARY_PTR: usize = 0x143942568;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const ALL_PART_LIBRARY_PTR: usize = 0x143a139f0;

const CITY_TYPE_OFFSET: usize = 0x25c;

// Offset from the city object to the category library node pointer.
#[cfg(feature = "1_151")]
const CATEGORY_NODE_OFFSET: usize = 0x69;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const CATEGORY_NODE_OFFSET: usize = 0x348;

// Address of pointer to the city object, used to resolve the category library (in R8)
// via +CATEGORY_NODE_OFFSET, and the city type (an i32 in 1–7) via +CITY_TYPE_OFFSET.
// We follow [CATEGORY_ROOT_PTR] -> +CATEGORY_NODE_OFFSET if non-null, else fall back to [CATEGORY_FALLBACK_PTR].
#[cfg(feature = "1_151")]
const CATEGORY_ROOT_PTR: usize = 0x1439220f0;
#[cfg(feature = "1_151")]
const CATEGORY_FALLBACK_PTR: usize = 0x147eed968;

#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const CATEGORY_ROOT_PTR: usize = 0x143a11940;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const CATEGORY_FALLBACK_PTR: usize = 0x147fc6f90;

// Hook address: right after a DefinePart call for MDL_ANTENNA_01 in the shop generation function.
// At 0x14029ae0f there is one instruction:
//   MOV ECX,dword ptr [RAX + 0x2a8]   (6 bytes: 8b 88 a8 02 00 00)
// This is 6 bytes, enough for a near jump. We save and replay it in the cave,
// and our injected function runs after the original DefinePart call has already completed.
#[cfg(feature = "1_151")]
const HOOK_ADDRESS: usize = 0x14029ae0f;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const HOOK_ADDRESS: usize = 0x1402bdf1c;

/// Patches the shop generation to include custom parts.
///
/// `parts` is a map of part model ID strings (e.g. `"MDL_WEAPON_01"`) to their
/// [`ShopPart`] configuration (probability, min/max count). Each time a shop is
/// generated the probability is rolled independently per part; if it passes, a
/// random count in `[min_parts, max_parts]` is chosen.
///
/// # Safety
/// Must be called while the game process memory is accessible and before the shop
/// generation function runs.
pub unsafe fn patch_custom_parts(parts: HashMap<String, Vec<ShopPart>>) {
    if parts.is_empty() {
        log::info!("No custom parts to inject, skipping patch.");
        return;
    }

    if HOOK_ADDRESS == 0x0 {
        log::warn!("Custom parts patching is not supported on this game version.");
        return;
    }

    // Convert to CustomPart structs with stable CString pointers.
    // Each part name can have multiple ShopPart entries (e.g. different
    // probabilities per city type), so we flatten them all into one list.
    let custom_parts: Vec<CustomPart> = parts
        .into_iter()
        .flat_map(|(name, cfgs)| {
            let cs = match CString::new(name.clone()) {
                Ok(cs) => cs,
                Err(e) => {
                    log::error!("Invalid part string '{}': {}", name, e);
                    return Vec::new();
                }
            };

            cfgs.into_iter()
                .filter_map({
                    let name = name.clone();
                    let cs = cs.clone();
                    move |cfg| {
                        let probability = cfg.probability.clamp(0.0, 1.0);
                        let min_parts = cfg.min_parts.max(1);
                        let max_parts = cfg.max_parts.max(min_parts);
                        log::info!(
                            "  Part '{}': probability={:.0}%, count=[{}, {}]",
                            name,
                            probability * 100.0,
                            min_parts,
                            max_parts,
                        );
                        let city_types = cfg.city_types.clone();
                        if !city_types.is_empty() {
                            log::info!("    city_types={:?}", city_types);
                        }
                        Some(CustomPart {
                            moid: cs.clone(),
                            probability,
                            min_parts,
                            max_parts,
                            city_types,
                        })
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    if custom_parts.is_empty() {
        log::warn!("All custom part strings were invalid, skipping patch.");
        return;
    }

    log::info!(
        "Patching shop generation to inject up to {} custom part type(s).",
        custom_parts.len()
    );

    // Seed the RNG once at init time.
    rng::seed();

    // SAFETY: We only write to CUSTOM_PARTS once during init, before any reads occur.
    CUSTOM_PARTS = custom_parts;

    // Hook after the existing DefinePart call.
    // save_overwritten = true ensures the original CALL instruction executes first,
    // then our function runs to inject the additional parts.
    let p = Patch::patch_call(
        HOOK_ADDRESS,
        inject_custom_parts as *const (),
        6,
        true,
        ReturnType::None,
    );
    std::mem::forget(p);
}

/// Reads the current city type from the city object.
///
/// Returns `Some(1..=7)` on success, or `None` if the city object pointer is null
/// or the value is outside the expected range.
unsafe fn read_city_type() -> Option<u32> {
    if CATEGORY_ROOT_PTR == 0x0 {
        return None;
    }
    let city_obj = *(CATEGORY_ROOT_PTR as *const *const u8);
    if city_obj.is_null() {
        return None;
    }
    let city_type = *((city_obj as usize + CITY_TYPE_OFFSET) as *const i32);
    if (1..=7).contains(&city_type) {
        Some(city_type as u32)
    } else {
        log::warn!("Unexpected city type value: {}", city_type);
        None
    }
}

/// Resolves the category library node pointer using the same logic as the game:
/// Try `[CATEGORY_ROOT_PTR]` -> `+CATEGORY_NODE_OFFSET`, fall back to `[CATEGORY_FALLBACK_PTR]`.
unsafe fn get_category_node() -> *const u8 {
    let root = *(CATEGORY_ROOT_PTR as *const *const u8);
    if !root.is_null() {
        let node = *((root as usize + CATEGORY_NODE_OFFSET) as *const *const u8);
        if !node.is_null() {
            return node;
        }
    }
    *(CATEGORY_FALLBACK_PTR as *const *const u8)
}

/// Called from the patch cave after the original DefinePart call.
/// Iterates over all custom parts, rolls probability, picks a random count,
/// and calls DefinePart for each part that passes the check.
///
/// On x86_64 Windows the standard calling convention (used by `__fastcall`)
/// passes the first four integer/pointer arguments in RCX, RDX, R8, R9,
/// which is exactly what `extern "C"` produces on this target.
unsafe extern "C" fn inject_custom_parts() {
    // Function pointer type matching DefinePart's signature on x64 Windows.
    // extern "C" on x86_64-pc-windows-msvc uses the Microsoft x64 ABI (RCX, RDX, R8, R9).
    type DefinePartFn = unsafe extern "C" fn(
        all_part_library: *const u8,
        moid: *const u8,
        category_library: *const u8,
        count: i32,
    ) -> *const u8;

    let define_part: DefinePartFn = std::mem::transmute(DEFINE_PART_FN as *const ());

    let all_part_library = *(ALL_PART_LIBRARY_PTR as *const *const u8);
    if all_part_library.is_null() {
        return;
    }

    let category_node = get_category_node();
    if category_node.is_null() {
        return;
    }

    // Read the current city type (re-read every call since the pointer may change).
    let city_type = read_city_type();

    // SAFETY: CUSTOM_PARTS is only written to once during init before this callback
    // can ever fire. After that it is effectively read-only.
    let parts_ptr = std::ptr::addr_of!(CUSTOM_PARTS);
    for part in (*parts_ptr).iter() {
        // If the part has a city_types filter, skip it when the current city doesn't match.
        if !part.city_types.is_empty() {
            match city_type {
                Some(ct) if part.city_types.contains(&ct) => { /* allowed */ }
                Some(_) => continue,
                None => {
                    // Could not determine city type; skip filtered parts to be safe.
                    continue;
                }
            }
        }

        let roll = rng::random_f32();
        if roll >= part.probability {
            continue;
        }

        let count = rng::random_range(part.min_parts, part.max_parts) as i32;

        let moid_ptr = part.moid.as_ptr() as *const u8;
        define_part(all_part_library, moid_ptr, category_node, count);
    }
}
