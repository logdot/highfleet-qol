//! Ports the Steam 1.151 part-floor rules and KAZ intercept cone to Steam 1.163.
//!
//! `classic` patches two 1.151 floor differences and the 1.151 KAZ intercept cone:
//!
//! 1. `World_RaycastBodies` (`0x140239f30`) adds one to the height level of some candidate bodies.
//!    These are bodies whose master is missing or of type 7 (free-standing parts: swivel guns, engines, legs).
//!    Steam 1.151 does not, so there such a part never blocks a gun on the same level.
//!    The `cmovz` that keeps the plain level for other bodies becomes a plain `mov`.
//! 2. `Ship_UpdatePartElevation` (`0x140256a90`) lifts a part that carries a gun by one level.
//!    It does so when something next to the part is higher, but tests only the gun category bit `0x20`.
//!    Steam 1.151 also accepted the engine bit `0x100`.
//!    The `GetPartFlagsWord` call in that test is detoured so the engine bit produces the same result.
//! 3. The KAZ intercept half-angle is pi/4 in 1.163 and 1.2 rad in 1.151.
//!    The two `movss` loads in `Weapon_FlakInterceptScan` are pointed at an existing 1.2f.
//!
//! The KAZ covered cone, threshold and sector ray start are `OL.seria` data and are not patched.
//! They are `m_mdl_sectors_type`, `m_mdl_sectors_threshold` and `m_mdl_sectors_shift`.

use patchy::{Condition, PatchSession, Result, Trampoline};

use crate::config::ElevationRules;

/// `lea edx,[rax+1]; cmovz edx,eax` in `World_RaycastBodies`; only the `cmovz` is replaced.
const CANDIDATE_SITE: usize = 0x14023a100;
/// `mov edx,eax; nop`.
const CANDIDATE_PATCH: [u8; 3] = [0x89, 0xc2, 0x90];

/// `call GetPartFlagsWord` in `Ship_UpdatePartElevation`, before `test al,0x20`.
const ELEVATION_FLAGS_CALL: usize = 0x140256d3e;
const ELEVATION_FLAGS_CALL_SIZE: usize = 5;
const GET_PART_FLAGS_WORD: usize = 0x1400457d0;

/// `movss xmm1,[rip+disp]` loading pi/4, in the two target loops of `Weapon_FlakInterceptScan`.
const CONE_SITES: [usize; 2] = [0x14002f864, 0x14002faf8];
/// An existing `1.2f` in `.rdata`.
const CONE_1151_ADDRESS: usize = 0x140354758;

/// `movss xmm1,[rip+disp32]` for an instruction at `at` reading `target`.
fn movss_xmm1(at: usize, target: usize) -> [u8; 8] {
    let disp = (target as i64 - (at as i64 + 8)) as i32;
    let mut bytes = [0xf3, 0x0f, 0x10, 0x0d, 0, 0, 0, 0];
    bytes[4..].copy_from_slice(&disp.to_le_bytes());
    bytes
}

/// Prepares the selected patches.
/// Does nothing for `vanilla` and `modern`.
pub unsafe fn patch_elevation_rules(session: &mut PatchSession, rules: ElevationRules) -> Result {
    if !rules.is_classic() {
        log::info!("Elevation rules are the game's own, leaving them alone");
        return Ok(());
    }

    patch_floors(session)?;
    patch_engine_elevation(session)?;
    patch_kaz_cone(session)
}

unsafe fn patch_floors(session: &mut PatchSession) -> Result {
    session.overwrite(CANDIDATE_SITE + 3, &CANDIDATE_PATCH)?;
    log::info!("Elevation rules: free-standing parts no longer block same-level guns");
    Ok(())
}

unsafe fn patch_engine_elevation(session: &mut PatchSession) -> Result {
    session.detour_trampoline(ELEVATION_FLAGS_CALL, ELEVATION_FLAGS_CALL_SIZE, |site| {
        let mut trampoline = Trampoline::new();
        let done = trampoline.new_label();
        trampoline.absolute_call(GET_PART_FLAGS_WORD as *const ());
        trampoline.bytes(&[0xa9, 0x00, 0x01, 0x00, 0x00]); // test eax, 0x100
        trampoline.jump_if(Condition::Equal, done);
        trampoline.bytes(&[0x83, 0xc8, 0x20]); // or eax, 0x20
        trampoline.bind(done)?;
        site.resume(&mut trampoline);
        Ok(trampoline)
    })?;
    log::info!("Elevation rules: engines receive adjacency elevation like 1.151");
    Ok(())
}

unsafe fn patch_kaz_cone(session: &mut PatchSession) -> Result {
    for at in CONE_SITES {
        session.overwrite(at, &movss_xmm1(at, CONE_1151_ADDRESS))?;
    }
    log::info!("Elevation rules: KAZ target cone widened to 1.2 rad");
    Ok(())
}
