//! Replaces the map cruise-speed formula (Steam 1.163 only).
//!
//! The ship stats code computes the speed in m/s as `50.0 * powf(TWR - 1.0, 0.5)`. The single
//! `call powf` at `0x140080699` is replaced with a call to [`speed_curve`], which evaluates the
//! configured curve and returns `speed / 50` because the game multiplies the result by its own
//! `50.0` constant afterwards.

use std::sync::OnceLock;

use patchy::{PatchSession, Result, ReturnType};

use crate::config::{SpeedConfig, SpeedCurve};

/// `E8 rel32`, the `call powf` that is replaced.
const CALL_SITE: usize = 0x140080699;
const CALL_SIZE: usize = 5;
/// Start of the byte window around the call that is checked before patching.
const WINDOW: usize = 0x14008068e;
/// The `50.0f` the game multiplies by after the call.
const FIFTY_ADDRESS: usize = 0x140352824;

const EXPECTED: [u8; 24] = [
    0xf3, 0x0f, 0x5c, 0xd7, // subss xmm2, xmm7        (TWR - 1)
    0x41, 0x0f, 0x28, 0xcb, // movaps xmm1, xmm11      (exponent 0.5)
    0x0f, 0x28, 0xc2, // movaps xmm0, xmm2
    0xe8, 0x68, 0x5f, 0x2b, 0x00, // call powf
    0xf3, 0x0f, 0x59, 0x05, 0x7e, 0x21, 0x2d, 0x00, // mulss xmm0, [50.0]
];

static CURVE: OnceLock<SpeedCurve> = OnceLock::new();

/// Called instead of `powf(TWR - 1, 0.5)`; both arguments arrive in `xmm0`/`xmm1`.
unsafe extern "C" fn speed_curve(twr_minus_one: f32, _exponent: f32) -> f32 {
    match CURVE.get() {
        Some(curve) => curve.speed_mps(twr_minus_one + 1.0) / 50.0,
        None => twr_minus_one.powf(0.5),
    }
}

/// Checks that the call site and the `50.0` constant are still what was analysed.
unsafe fn site_matches() -> bool {
    let window = std::slice::from_raw_parts(WINDOW as *const u8, EXPECTED.len());
    window == EXPECTED && (FIFTY_ADDRESS as *const f32).read_unaligned() == 50.0
}

/// Prepares the speed curve patch. Does nothing while the curve is the game's own.
pub unsafe fn patch_speed(session: &mut PatchSession, config: &SpeedConfig) -> Result {
    let Some(curve) = config.curve() else {
        log::error!(
            "Speed curve: invalid values in the \"speed\" section, keeping the game's curve"
        );
        return Ok(());
    };

    if curve.is_vanilla() {
        log::info!("Speed curve is the game's own, leaving it alone");
        return Ok(());
    }

    if !site_matches() {
        log::error!(
            "Speed curve: code at {CALL_SITE:#x} is not what was expected, leaving it alone"
        );
        return Ok(());
    }

    let _ = CURVE.set(curve);
    session.patch_call(
        CALL_SITE,
        speed_curve as *const (),
        CALL_SIZE,
        false,
        ReturnType::Xmm0,
    )?;
    log::info!(
        "Speed curve patch prepared at {CALL_SITE:#x} ({} * max(TWR - {}, 0) ^ {})",
        curve.coefficient,
        curve.subtract,
        curve.exponent
    );
    Ok(())
}
