//! Ports the Steam 1.151 firing-arc rules to Steam 1.163 (the `classic` elevation rules).
//!
//! Three independent differences:
//!
//! 1. `World_RaycastBodies` (`0x140239f30`) adds one to the height level of a candidate body whose
//!    master is missing or of type 7 (free-standing parts: swivel guns, engines, legs). Steam
//!    1.151 does not, so there such a part never blocks a gun on the same level. The `cmovz` that
//!    keeps the plain level for other bodies becomes a plain `mov`.
//! 2. The KAZ module accepts a target when the bearing is below a half-angle: 1.2 rad in 1.151,
//!    pi/4 in 1.163. Two `movss` loads in `Weapon_FlakInterceptScan` are pointed at a 1.2f.
//! 3. The KAZ is blocked when part of its own 360 sectors is covered. 1.151 looked at the rear
//!    34 degrees (sector type 3) with a 50% threshold; the 1.163 data uses type 5 (rear 90
//!    degrees) with 15%. Type 5 is only used by the KAZ, so in `UpdateEngineCovered` the type 5
//!    branch is replaced by "load 0.5 as the threshold, then take the type 3 branch".

use patchy::{PatchSession, Result};

use crate::config::ElevationRules;

/// `lea edx,[rax+1]; cmovz edx,eax` in `World_RaycastBodies`; only the `cmovz` is replaced.
const CANDIDATE_SITE: usize = 0x14023a100;
/// `mov edx,eax; nop`.
const CANDIDATE_PATCH: [u8; 3] = [0x89, 0xc2, 0x90];

/// `movss xmm1,[rip+disp]` loading pi/4, in the two target loops of `Weapon_FlakInterceptScan`.
const CONE_SITES: [usize; 2] = [0x14002f864, 0x14002faf8];
/// An existing `1.2f` in `.rdata`.
const CONE_1151_ADDRESS: usize = 0x140354758;

/// The type 5 branch of `UpdateEngineCovered`: `comiss xmm6,xmm14; jc ...; movss xmm0,[...];
/// comiss xmm0,xmm6`, ending right before the shared `jc` at `0x14001c69b`.
const COVERED_SITE: usize = 0x14001c68a;
const COVERED_SIZE: usize = 17;
/// The type 3 branch the new code jumps to (`comiss xmm6,xmm12`).
const COVERED_TYPE_3: usize = 0x14001c65f;
/// A `0.5f` in `.rdata`.
const HALF_ADDRESS: usize = 0x1403527a4;

/// `movss xmm1,[rip+disp32]` for an instruction at `at` reading `target`.
fn movss_xmm1(at: usize, target: usize) -> [u8; 8] {
    let disp = (target as i64 - (at as i64 + 8)) as i32;
    let mut bytes = [0xf3, 0x0f, 0x10, 0x0d, 0, 0, 0, 0];
    bytes[4..].copy_from_slice(&disp.to_le_bytes());
    bytes
}

/// The replacement for the type 5 branch: `movss xmm10,[0.5f]; jmp type3; nop...`.
fn covered_patch() -> [u8; COVERED_SIZE] {
    let mut bytes = [0x90; COVERED_SIZE];
    let disp = (HALF_ADDRESS as i64 - (COVERED_SITE as i64 + 9)) as i32;
    bytes[..5].copy_from_slice(&[0xf3, 0x44, 0x0f, 0x10, 0x15]);
    bytes[5..9].copy_from_slice(&disp.to_le_bytes());
    let rel = (COVERED_TYPE_3 as i64 - (COVERED_SITE as i64 + 11)) as i8;
    bytes[9] = 0xeb;
    bytes[10] = rel as u8;
    bytes
}

/// Prepares the classic firing-arc patches. Does nothing for `vanilla` and `modern`.
pub unsafe fn patch_elevation_rules(session: &mut PatchSession, rules: ElevationRules) -> Result {
    if !rules.is_classic() {
        log::info!("Elevation rules are the game's own, leaving them alone");
        return Ok(());
    }

    session.overwrite(CANDIDATE_SITE + 3, &CANDIDATE_PATCH)?;
    log::info!("Elevation rules: free-standing parts no longer block same-level guns");

    for at in CONE_SITES {
        session.overwrite(at, &movss_xmm1(at, CONE_1151_ADDRESS))?;
    }
    log::info!("Elevation rules: KAZ target cone widened to 1.2 rad");

    session.overwrite(COVERED_SITE, &covered_patch())?;
    log::info!("Elevation rules: KAZ covered test uses the 1.151 rear cone and 50% threshold");
    Ok(())
}
