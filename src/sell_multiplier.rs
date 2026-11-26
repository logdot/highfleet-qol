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

use patchy::{CallBridge, FlagPolicy, KeepReturn, PatchSession, Result, StackAlignment};

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

/// Prepares the sell-multiplier patch if the multiplier differs from 1.0.
pub unsafe fn patch_sell_multiplier(session: &mut PatchSession, multiplier: f32) -> Result {
    if (multiplier - 1.0).abs() < f32::EPSILON {
        log::info!("Sell multiplier is 1.0, skipping patch.");
        return Ok(());
    }

    if PATCH_ADDRESS == 0x0 {
        log::warn!("Sell multiplier patch is not supported on this game version.");
        return Ok(());
    }

    SELL_MULTIPLIER = multiplier;

    session.patch_call(
        PATCH_ADDRESS,
        PATCH_SIZE,
        CallBridge {
            callback: apply_sell_multiplier as *const (),
            incoming_stack: StackAlignment::Aligned16,
            flags: FlagPolicy::Preserve,
            keep_return: KeepReturn::Neither,
            before_call: &PASS_PROFILE_AND_PRICE,
            after_call: &[],
        },
    )?;

    log::info!(
        "Sell multiplier patch prepared at {PATCH_ADDRESS:#x} (multiplier: {multiplier:.2}x)"
    );
    Ok(())
}

/// Moves the live profile pointer into RCX and the sale price into EDX for [`apply_sell_multiplier`].
///
/// The bridge's `before_call` still sees the site's registers:
///   v1.151: `MOV RCX, RAX; MOV EDX, EBX`
///   v1.163: `MOV EDX, ECX; MOV RCX, RAX` (the price is read before RCX is overwritten)
#[cfg(feature = "1_151")]
const PASS_PROFILE_AND_PRICE: [u8; 5] = [0x48, 0x89, 0xc1, 0x89, 0xda];
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
const PASS_PROFILE_AND_PRICE: [u8; 5] = [0x89, 0xca, 0x48, 0x89, 0xc1];

/// Pure-Rust helper that applies the sell multiplier.
///
/// Receives the profile pointer and price as function arguments in RCX and EDX.
unsafe extern "C" fn apply_sell_multiplier(profile_ptr: *mut u8, price: i32) {
    let cash_ptr = profile_ptr.add(CASH_OFFSET) as *mut i32;
    let adjusted_price = (price as f32 * SELL_MULTIPLIER) as i32;
    let new_cash = cash_ptr.read() + adjusted_price;
    cash_ptr.write(new_cash);
}
