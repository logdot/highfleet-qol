#[cfg(any(not(feature = "1_151"), test))]
use patchy::{
    CallBridge, Condition, DetourSite, FlagPolicy, KeepReturn, StackAlignment, Trampoline,
};
use patchy::{PatchSession, Result};

#[cfg(any(not(feature = "1_151"), test))]
mod sector_blocking;
#[cfg(any(not(feature = "1_151"), test))]
mod fire_gun {
    //! FireGun addresses in v1.163.

    /// `SUBSS XMM1, XMM7; COMISS XMM6, XMM1`, at the charge decrement.
    pub const INJECTION_ADDR: usize = 0x140032f22;
    pub const INJECTION_SIZE: usize = 7;
    /// The path that leaves FireGun without firing.
    pub const EXIT_0_ADDR: usize = 0x140032ef0;
}

/// In v1.151, gun blocking already exists in the game.
/// This function NOPs out the blocking check to allow guns to fire through own ship.
#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_blocking(session: &mut PatchSession) -> Result {
    let address: usize = 0x14003314d;
    let size: usize = 6;

    let data = vec![0x90; size]; // NOP instructions
    session.overwrite(address, &data)?;
    Ok(())
}

/// Gun blocking is already absent in v1.163, so "unblocking" is a no-op.
#[cfg(not(feature = "1_151"))]
pub unsafe fn patch_sector_blocking(_session: &mut PatchSession) -> Result {
    Ok(())
}

/// In v1.151, gun blocking is native — nothing to restore.
#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_restoration(_session: &mut PatchSession) -> Result {
    Ok(())
}

/// Restores the gun-blocking sector check into v1.163's FireGun function.
///
/// In v1.151, FireGun contained logic that checked a 360-float "sectors" array on each gun body to determine if the gun's firing arc was blocked by its own ship.
/// This check was removed in v1.163.
/// This patch re-implements it by:
///
/// 1. Writing an `is_gun_blocked` function in Rust
/// 2. Detouring the charge-decrement point to a trampoline that calls the Rust function
/// 3. Leaving FireGun without firing if blocked, otherwise replaying the charge decrement
#[cfg(not(feature = "1_151"))]
pub unsafe fn patch_sector_restoration(session: &mut PatchSession) -> Result {
    use fire_gun::{INJECTION_ADDR, INJECTION_SIZE};

    let p = session.detour_trampoline_with(INJECTION_ADDR, INJECTION_SIZE, |site| {
        build_gun_blocking(site, sector_blocking::is_gun_blocked as *const ())
    })?;
    let cave_base = p
        .trampoline_address()
        .expect("gun-blocking detour has no trampoline");

    log::info!("gun_blocking: trampoline prepared at {INJECTION_ADDR:#x} → cave at {cave_base:#x}");
    Ok(())
}

/// Builds the FireGun trampoline around the `is_blocked(gun: *const Body) -> bool` predicate.
///
/// At the injection point, register state is:
///   RDI  = param_1 (Body pointer, non-volatile)
///   XMM1 = fFireCharge (volatile, saved by the bridge)
///   XMM6 = 0.0  (non-volatile)
///   XMM7 = 1.0  (non-volatile)
///   RSP  ≡ 0 mod 16
///
/// The bridge leaves the predicate's `TEST AL, AL` flags for the branch.
/// A blocked gun jumps to EXIT_0 without the charge decrement.
/// Otherwise the replayed `COMISS` produces the flags FireGun consumes after the site.
#[cfg(any(not(feature = "1_151"), test))]
fn build_gun_blocking(site: DetourSite, is_blocked: *const ()) -> Result<Trampoline> {
    let mut cave = Trampoline::new();
    let blocked = cave.new_label();

    // SAFETY: RSP is 16-byte aligned at the site, the predicate takes its only argument in RCX,
    // and both byte slices are straight-line and position-independent.
    unsafe {
        cave.call(CallBridge {
            callback: is_blocked,
            incoming_stack: StackAlignment::Aligned16,
            flags: FlagPolicy::KeepProduced,
            keep_return: KeepReturn::Neither,
            before_call: &[0x48, 0x89, 0xf9], // MOV RCX, RDI
            after_call: &[0x84, 0xc0],        // TEST AL, AL
        });
    }
    cave.jump_if(Condition::NotEqual, blocked);

    // SAFETY: SUBSS and COMISS between registers do not depend on their address.
    unsafe { site.replay_verbatim(&mut cave) };
    site.resume(&mut cave);

    cave.bind(blocked)?;
    cave.relative_jump(fire_gun::EXIT_0_ADDR);
    Ok(cave)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fire_gun::{EXIT_0_ADDR, INJECTION_ADDR, INJECTION_SIZE};

    const CAVE: usize = 0x140010000;
    const CHARGE_DECREMENT: [u8; INJECTION_SIZE] = [0xf3, 0x0f, 0x5c, 0xcf, 0x0f, 0x2f, 0xf1];
    const JNZ: [u8; 2] = [0x0f, 0x85];
    const JMP: u8 = 0xe9;

    fn built_cave() -> Vec<u8> {
        build_gun_blocking(
            DetourSite::new(INJECTION_SIZE),
            0x1_8000_1000usize as *const (),
        )
        .unwrap()
        .into_plan()
        .unwrap()
        .build_for_site(CAVE, &CHARGE_DECREMENT, INJECTION_ADDR + INJECTION_SIZE)
        .unwrap()
    }

    fn jump_target(code: &[u8], displacement_at: usize) -> usize {
        let displacement = i32::from_le_bytes(code[displacement_at..][..4].try_into().unwrap());
        (CAVE + displacement_at + 4).wrapping_add_signed(displacement as isize)
    }

    #[test]
    fn blocked_guns_skip_the_charge_decrement_and_leave_through_exit_0() {
        let code = built_cave();
        let exit = code.len() - 5;
        let branch = exit - 5 - INJECTION_SIZE - 6;

        assert_eq!(&code[branch..branch + 2], &JNZ);
        assert_eq!(jump_target(&code, branch + 2), CAVE + exit);
        assert_eq!(code[exit], JMP);
        assert_eq!(jump_target(&code, exit + 1), EXIT_0_ADDR);
    }

    #[test]
    fn unblocked_guns_replay_the_charge_decrement_and_resume_after_it() {
        let code = built_cave();
        let resume = code.len() - 10;
        let replay = resume - INJECTION_SIZE;

        assert_eq!(&code[replay..resume], &CHARGE_DECREMENT);
        assert_eq!(code[resume], JMP);
        assert_eq!(
            jump_target(&code, resume + 1),
            INJECTION_ADDR + INJECTION_SIZE
        );
    }
}
