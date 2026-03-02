//! Patches the SomethingWithLevelSerias function to inject custom parts into generated shops.
//!
//! This module hooks after an existing DefinePart call in the weapons category section
//! of the shop generation function, and calls DefinePart for each user-configured part string.

use std::ffi::CString;

use crate::patchy::{Patch, ReturnType};

/// The list of custom part strings to inject into weapon shops.
static mut CUSTOM_PARTS: Vec<CString> = Vec::new();

// DefinePart function address
// Body * __fastcall DefinePart(Body * allPartLibrary, char * moid, Node * categoryLibrary, int count)
// On x86_64 Windows, __fastcall is the standard calling convention (RCX, RDX, R8, R9).
#[cfg(feature = "1_151")]
const DEFINE_PART_FN: usize = 0x1401fde40;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const DEFINE_PART_FN: usize = 0x0;

// Address of pointer to allPartLibrary (first arg to DefinePart, in RCX)
#[cfg(feature = "1_151")]
const ALL_PART_LIBRARY_PTR: usize = 0x143942568;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const ALL_PART_LIBRARY_PTR: usize = 0x0;

// Address of pointer to the root node used to resolve the category library (in R8)
// We follow [0x1439220f0] -> +0x348 if non-null, else fall back to [0x147eed968]
#[cfg(feature = "1_151")]
const CATEGORY_ROOT_PTR: usize = 0x1439220f0;
#[cfg(feature = "1_151")]
const CATEGORY_FALLBACK_PTR: usize = 0x147eed968;

#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const CATEGORY_ROOT_PTR: usize = 0x0;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const CATEGORY_FALLBACK_PTR: usize = 0x0;

// Hook address: right after a DefinePart call for MDL_ANTENNA_01 in the shop generation function.
// At 0x14029ae0f there is one instruction:
//   MOV ECX,dword ptr [RAX + 0x2a8]   (6 bytes: 8b 88 a8 02 00 00)
// This is 6 bytes, enough for a near jump. We save and replay it in the cave,
// and our injected function runs after the original DefinePart call has already completed.
#[cfg(feature = "1_151")]
const HOOK_ADDRESS: usize = 0x14029ae0f;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const HOOK_ADDRESS: usize = 0x0;

/// Patches the shop generation to include custom parts.
///
/// `parts` is a list of part model ID strings (e.g. `"MDL_WEAPON_01"`) that will be
/// added to weapon shops via DefinePart with a count of 1.
///
/// # Safety
/// Must be called while the game process memory is accessible and before the shop
/// generation function runs.
pub unsafe fn patch_custom_parts(parts: Vec<String>) {
    if parts.is_empty() {
        log::info!("No custom parts to inject, skipping patch.");
        return;
    }

    if HOOK_ADDRESS == 0x0 {
        log::warn!("Custom parts patching is not supported on this game version.");
        return;
    }

    // Convert to CStrings so we have stable null-terminated pointers
    let custom_parts: Vec<CString> = parts
        .into_iter()
        .filter_map(|s| match CString::new(s.clone()) {
            Ok(cs) => Some(cs),
            Err(e) => {
                log::error!("Invalid part string '{}': {}", s, e);
                None
            }
        })
        .collect();

    if custom_parts.is_empty() {
        log::warn!("All custom part strings were invalid, skipping patch.");
        return;
    }

    log::info!(
        "Patching shop generation to inject {} custom part(s).",
        custom_parts.len()
    );

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

/// Resolves the category library node pointer using the same logic as the game:
/// Try `[CATEGORY_ROOT_PTR]` -> `+0x348`, fall back to `[CATEGORY_FALLBACK_PTR]`.
unsafe fn get_category_node() -> *const u8 {
    let root = *(CATEGORY_ROOT_PTR as *const *const u8);
    if !root.is_null() {
        let node = *((root as usize + 0x348) as *const *const u8);
        if !node.is_null() {
            return node;
        }
    }
    *(CATEGORY_FALLBACK_PTR as *const *const u8)
}

/// Called from the patch cave after the original DefinePart call.
/// Iterates over all custom parts and calls DefinePart for each one.
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

    // SAFETY: CUSTOM_PARTS is only written to once during init before this callback
    // can ever fire. After that it is effectively read-only.
    let parts_ptr = std::ptr::addr_of!(CUSTOM_PARTS);
    for part in (*parts_ptr).iter() {
        let moid_ptr = part.as_ptr() as *const u8;
        define_part(all_part_library, moid_ptr, category_node, 10);
    }
}
