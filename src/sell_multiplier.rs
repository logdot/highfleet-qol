//! Patches `TransferShopItemToPlayer` to apply a configurable multiplier to
//! the money received when selling a part.
//!
//! At the patch point the register state is:
//!   RAX = profile pointer (from `Get_Profile()`)
//!   v1.151: EBX = computed sale price (integer)
//!   v1.163: ECX = computed sale price (already float-multiplied by param_4)
//!
//! The original instruction adds the price to the player's cash:
//!   v1.151: ADD dword ptr [RAX + 0x260], EBX
//!   v1.163: ADD dword ptr [RAX + 0x2a8], ECX
//!
//! We replace it with our own logic that multiplies the price by the configured
//! multiplier before adding it to the player's cash.

use std::arch::naked_asm;

use crate::patchy::{Patch, ReturnType};

/// The sell price multiplier, written once at init before any patch callback fires.
static mut SELL_MULTIPLIER: f32 = 1.0;

/// The byte offset of `cash` within the Profile struct (version-dependent).
#[cfg(feature = "1_151")]
const CASH_OFFSET: usize = 0x260;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const CASH_OFFSET: usize = 0x2a8;

/// Address of the ADD instruction inside TransferShopItemToPlayer.
///   v1.151: ADD dword ptr [RAX + 0x260], EBX   (6 bytes)
///   v1.163: ADD dword ptr [RAX + 0x2a8], ECX   (6 bytes)
#[cfg(feature = "1_151")]
const PATCH_ADDRESS: usize = 0x140204467;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const PATCH_ADDRESS: usize = 0x140221a3f;

/// Size of the overwritten instruction (6 bytes in both versions).
const PATCH_SIZE: usize = 6;

/// Installs the sell-multiplier patch if the multiplier differs from 1.0.
pub unsafe fn patch_sell_multiplier(multiplier: f32) {
    if (multiplier - 1.0).abs() < f32::EPSILON {
        log::info!("Sell multiplier is 1.0, skipping patch.");
        return;
    }

    if PATCH_ADDRESS == 0x0 {
        log::warn!("Sell multiplier patch is not supported on this game version.");
        return;
    }

    SELL_MULTIPLIER = multiplier;

    let p = Patch::patch_call(
        PATCH_ADDRESS,
        trampoline as *const (),
        PATCH_SIZE,
        false,
        ReturnType::None,
    );
    std::mem::forget(p);

    log::info!(
        "Sell multiplier patch installed at {PATCH_ADDRESS:#x} (multiplier: {multiplier:.2}x)"
    );
}

/// Naked trampoline that shuttles live register values into the Windows x64
/// calling convention (RCX, RDX) and tail-calls the pure-Rust helper.
///
/// On entry (after patchy's register save sequence):
///   RAX = profile pointer  (not clobbered — patchy pushes it but doesn't modify it)
///   v1.151: EBX = sale price (non-volatile, untouched by patchy)
///   v1.163: ECX = sale price (saved/restored by patchy as a volatile register,
///           but the value is still live in RCX at our entry point because patchy
///           pushes RCX early in SAVE_REGISTERS and the push doesn't modify it)
///
/// We move the profile pointer into RCX and the price into EDX, then JMP to
/// the helper.
#[unsafe(naked)]
#[cfg(feature = "1_151")]
unsafe extern "C" fn trampoline() {
    naked_asm!(
        "mov rcx, rax",
        "mov edx, ebx",
        "jmp {helper}",
        helper = sym apply_sell_multiplier,
    );
}

#[unsafe(naked)]
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
unsafe extern "C" fn trampoline() {
    naked_asm!(
        "mov edx, ecx",
        "mov rcx, rax",
        "jmp {helper}",
        helper = sym apply_sell_multiplier,
    );
}

/// Pure-Rust helper that applies the sell multiplier.
///
/// Receives the profile pointer and price directly as function arguments
/// (forwarded from the trampoline via RCX and EDX).
unsafe extern "C" fn apply_sell_multiplier(profile_ptr: *mut u8, price: i32) {
    let cash_ptr = profile_ptr.add(CASH_OFFSET) as *mut i32;
    let adjusted_price = (price as f32 * SELL_MULTIPLIER) as i32;
    let new_cash = cash_ptr.read() + adjusted_price;
    cash_ptr.write(new_cash);
}
