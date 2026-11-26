//! Replaces the map cruise-speed formula (Steam 1.163 only).
//!
//! The ship stats code computes the speed in m/s as `50.0 * powf(TWR - 1.0, 0.5)`. The single
//! `call powf` at `0x140080699` is replaced with a call to [`speed_curve`], which evaluates the
//! configured curve and returns `speed / 50` because the game multiplies the result by its own
//! `50.0` constant afterwards.

use std::sync::OnceLock;

use patchy::{CallBridge, FlagPolicy, KeepReturn, PatchSession, Result, StackAlignment};

use crate::config::{SpeedConfig, SpeedCurve};

/// `E8 rel32`, the `call powf` that is replaced.
/// It is preceded by `subss xmm2, xmm7` (TWR - 1), `movaps xmm1, xmm11` (exponent 0.5),
/// and `movaps xmm0, xmm2`, and followed by `mulss xmm0, [50.0]`.
const CALL_SITE: usize = 0x140080699;
const CALL_SIZE: usize = 5;

static CURVE: OnceLock<SpeedCurve> = OnceLock::new();

/// Called instead of `powf(TWR - 1, 0.5)`; both arguments arrive in `xmm0`/`xmm1`.
unsafe extern "C" fn speed_curve(twr_minus_one: f32, _exponent: f32) -> f32 {
    match CURVE.get() {
        Some(curve) => curve.speed_mps(twr_minus_one + 1.0) / 50.0,
        None => twr_minus_one.powf(0.5),
    }
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

    let _ = CURVE.set(curve);
    session.patch_call(
        CALL_SITE,
        CALL_SIZE,
        CallBridge {
            callback: speed_curve as *const (),
            incoming_stack: StackAlignment::Aligned16,
            flags: FlagPolicy::Preserve,
            keep_return: KeepReturn::Xmm0,
            before_call: &[],
            after_call: &[],
        },
    )?;
    log::info!(
        "Speed curve patch prepared at {CALL_SITE:#x} ({} * max(TWR - {}, 0) ^ {})",
        curve.coefficient,
        curve.subtract,
        curve.exponent
    );
    Ok(())
}
