//! Restores HighFleet 1.151's nullable missile-fuze link check in version 1.163.

#[cfg(any(not(feature = "1_151"), test))]
use patchy::{Condition, DetourSite, ProcessModule, Trampoline};
use patchy::{PatchSession, Result};

#[cfg(any(not(feature = "1_151"), test))]
const HOOK_RVA: usize = 0x36bb5;
/// `MOV RBX, RAX; MOV RCX, RSI`.
#[cfg(any(not(feature = "1_151"), test))]
const HOOK_SIZE: usize = 6;
#[cfg(any(not(feature = "1_151"), test))]
const NULL_EXIT_RVA: usize = 0x36ca2;

/// Prepares the nullable missile-fuze link check required by HighFleet 1.163.
#[cfg(not(feature = "1_151"))]
pub unsafe fn patch_flare_crash(session: &mut PatchSession) -> Result {
    let module = ProcessModule::main()?;
    let hook_address = module.resolve_rva(HOOK_RVA)?;

    session.detour_trampoline(hook_address, HOOK_SIZE, |site| {
        build_trampoline(site, module)
    })?;

    log::info!("Prepared flare crash fix");
    Ok(())
}

/// Reports that the 1.163-only fix is unnecessary on HighFleet 1.151.
#[cfg(feature = "1_151")]
pub unsafe fn patch_flare_crash(_session: &mut PatchSession) -> Result {
    log::info!("Flare crash fix is not required on HighFleet 1.151");
    Ok(())
}

#[cfg(any(not(feature = "1_151"), test))]
fn build_trampoline(site: DetourSite, module: ProcessModule) -> Result<Trampoline> {
    let mut trampoline = Trampoline::new();
    let non_null = trampoline.new_label();

    trampoline.bytes(&[0x48, 0x85, 0xc0]); // TEST RAX, RAX
    trampoline.jump_if(Condition::NotEqual, non_null);
    trampoline.absolute_jump(module.resolve_rva(NULL_EXIT_RVA)?);

    trampoline.bind(non_null)?;
    // SAFETY: Register-to-register moves do not depend on their address.
    unsafe { site.replay_verbatim(&mut trampoline) };
    site.resume(&mut trampoline);

    Ok(trampoline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_link_exits_while_non_null_link_replays_and_resumes() {
        let module = ProcessModule::from_base(0x140000000);
        let hook = module.resolve_rva(HOOK_RVA).unwrap();
        let snapshot = [0x48, 0x8b, 0xd8, 0x48, 0x8b, 0xce];
        let cave = 0x140010000;
        let trampoline = build_trampoline(DetourSite::new(HOOK_SIZE), module)
            .unwrap()
            .into_plan()
            .unwrap()
            .build_for_site(cave, &snapshot, hook + HOOK_SIZE)
            .unwrap();

        let branch = trampoline
            .strip_prefix(&[0x48, 0x85, 0xc0, 0x0f, 0x85]) // TEST RAX,RAX; JNZ rel32
            .expect("a non-null RAX must take the conditional branch");
        let (non_null_disp, fallthrough) = branch.split_at(4);
        let non_null = (trampoline.len() - fallthrough.len())
            .wrapping_add_signed(i32::from_le_bytes(non_null_disp.try_into().unwrap()) as isize);

        // The null path falls through to the jump out of the native function.
        let null_jump = fallthrough.strip_prefix(&[0xff, 0x25]).unwrap(); // JMP [RIP+disp32]
        let (pointer_disp, after_jump) = null_jump.split_at(4);
        let pointer = (trampoline.len() - after_jump.len())
            .wrapping_add_signed(i32::from_le_bytes(pointer_disp.try_into().unwrap()) as isize);
        assert_eq!(
            usize::from_le_bytes(trampoline[pointer..][..8].try_into().unwrap()),
            module.resolve_rva(NULL_EXIT_RVA).unwrap()
        );

        let resume = trampoline[non_null..]
            .strip_prefix(&snapshot)
            .expect("the non-null path must replay the displaced instructions")
            .strip_prefix(&[0xe9]) // JMP rel32
            .unwrap();
        let resume_disp = i32::from_le_bytes(resume[..4].try_into().unwrap());
        let next = cave + trampoline.len() - resume.len() + 4;
        assert_eq!(
            next.wrapping_add_signed(resume_disp as isize),
            hook + HOOK_SIZE
        );
    }
}
