//! Changes how fast fleets refuel at stations (Steam 1.163 only).
//!
//! In the fleet update function a docked fleet takes
//! `min(free tank space, dt * RATE, station stock)` of fuel each frame. `RATE` is a float
//! constant read by three `MULSS xmm0, [rip+disp32]` instructions:
//!
//! | Address        | Who                                 | Rate  |
//! |----------------|-------------------------------------|-------|
//! | `0x14011521f`  | player at a fuel station (type 4)   | 200.0 |
//! | `0x140115240`  | player at any other station         | 100.0 |
//! | `0x14011529c`  | AI fleets                           | 100.0 |
//!
//! The constants are shared with many unrelated places, so each instruction is replaced with a
//! call to a hook that returns `dt * base * multiplier`. The amount is still capped by free tank
//! space and station stock, so a large multiplier cannot over-deliver.

use std::sync::atomic::{AtomicU32, Ordering};

use patchy::{CallBridge, FlagPolicy, KeepReturn, PatchSession, Result, StackAlignment};

use crate::config::RefuelConfig;

/// Size of the `MULSS xmm0, [rip+disp32]` instruction that is replaced.
const MULSS_SIZE: usize = 8;

struct RateSite {
    address: usize,
    /// The vanilla rate constant the replaced `MULSS` reads.
    base: f32,
    hook: unsafe extern "C" fn(f32) -> f32,
    rate: &'static AtomicU32,
    player: bool,
}

static PLAYER_STATION_RATE: AtomicU32 = AtomicU32::new(0);
static PLAYER_OTHER_RATE: AtomicU32 = AtomicU32::new(0);
static AI_RATE: AtomicU32 = AtomicU32::new(0);

fn rate(slot: &AtomicU32) -> f32 {
    f32::from_bits(slot.load(Ordering::Relaxed))
}

unsafe extern "C" fn player_station_hook(dt: f32) -> f32 {
    dt * rate(&PLAYER_STATION_RATE)
}

unsafe extern "C" fn player_other_hook(dt: f32) -> f32 {
    dt * rate(&PLAYER_OTHER_RATE)
}

unsafe extern "C" fn ai_hook(dt: f32) -> f32 {
    dt * rate(&AI_RATE)
}

const SITES: [RateSite; 3] = [
    RateSite {
        address: 0x14011521f,
        base: 200.0,
        hook: player_station_hook,
        rate: &PLAYER_STATION_RATE,
        player: true,
    },
    RateSite {
        address: 0x140115240,
        base: 100.0,
        hook: player_other_hook,
        rate: &PLAYER_OTHER_RATE,
        player: true,
    },
    RateSite {
        address: 0x14011529c,
        base: 100.0,
        hook: ai_hook,
        rate: &AI_RATE,
        player: false,
    },
];

/// Prepares the refuel rate patches. Does nothing unless at least one multiplier differs from
/// `1.0`.
pub unsafe fn patch_refuel(session: &mut PatchSession, config: &RefuelConfig) -> Result {
    let player = config.player_rate_multiplier;
    let ai = config.ai_rate_multiplier;

    for site in &SITES {
        let multiplier = if site.player { player } else { ai };
        if multiplier == 1.0 {
            continue;
        }

        site.rate
            .store((site.base * multiplier).to_bits(), Ordering::Relaxed);
        session.patch_call(
            site.address,
            MULSS_SIZE,
            CallBridge {
                callback: site.hook as *const (),
                incoming_stack: StackAlignment::Aligned16,
                flags: FlagPolicy::Preserve,
                keep_return: KeepReturn::Xmm0,
                before_call: &[],
                after_call: &[],
            },
        )?;
        log::info!(
            "Refuel rate patch prepared at {:#x} ({} -> {})",
            site.address,
            site.base,
            site.base * multiplier
        );
    }

    Ok(())
}
