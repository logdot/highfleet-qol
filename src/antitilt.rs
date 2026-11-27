//! Makes the engine anti-tilt torque independent of the frame rate.
//!
//! `m_mdl_at` in `Libraries/OL.seria` drives the per-engine anti-tilt torque in `creatureLogic`'s
//! `logic id 200` case. The case runs once per frame and ends by calling `Body_AddTorque` on the
//! hull Frame. `Body_AddTorque` changes the hull's angular velocity immediately and is not
//! multiplied by the frame's `dt`, so the anti-tilt work done per second scales with the frame
//! rate: at a modified 100 fps the hull is pushed back to level twice as hard as at the vanilla
//! 50 fps.
//!
//! Steam 1.163 hooks the **entry of `Body_AddTorque`** rather than the stabiliser case, and only
//! scales the torque when the caller is the stabiliser (the return address of the
//! `CALL [RAX+0x1A0]` at `0x1400357cb`, i.e. `0x1400357d1`). The trampoline multiplies `XMM1` by
//! `dt * 50` (exactly `1.0` at the vanilla 50 fps, `0.5` at 100 fps), replays the stolen entry
//! bytes and jumps back into the function body. Hooking the torque entry rather than the case
//! keeps the case's own instructions free, so a mod that retargets the angle term there
//! (`hf_wish_tilt`) composes with this factor instead of replacing it.
//!
//! Steam 1.151 still hooks the case's `mov rax,[rdi]; mov rcx,rdi; call [rax+slot]` block, which
//! no 1.151 mod retargets.
//!
//! The frame `dt` is the game clock at `time_related_global_`, which is `1 / CORE_FRAMERATE`
//! times `CORE_TIMEWRAP`. Using it also keeps the anti-tilt consistent if the time wrap is
//! changed. Disabling `enable_antitilt` in `qol.json` leaves the game's own behaviour untouched
//! and lets `hf_wish_tilt` apply the factor itself.

use patchy::{PatchSession, Result};

/// `50.0f` (`0x42480000`), the vanilla `CORE_FRAMERATE` the anti-tilt behaviour is defined for.
const VANILLA_FRAMERATE: u32 = 0x4248_0000;

#[cfg(not(feature = "1_151"))]
mod v1_163 {
    use patchy::Trampoline;

    /// `Body_AddTorque` entry: `PUSH RDI; SUB RSP,0xC0` (9 bytes, no branch, no RIP-relative
    /// operand, so they relocate unchanged into the trampoline).
    pub(super) const HOOK: usize = 0x1_4001_9be0;
    pub(super) const STOLEN_SIZE: usize = 9;
    pub(super) const EXPECTED: [u8; STOLEN_SIZE] = [
        0x40, 0x57, // push rdi
        0x48, 0x81, 0xEC, 0xC0, 0x00, 0x00, 0x00, // sub rsp,0xC0
    ];
    /// The first byte of `Body_AddTorque`'s body (after the stolen prologue).
    pub(super) const CONTINUATION: usize = HOOK + STOLEN_SIZE;
    /// The instruction after the stabiliser's `CALL [RAX+0x1A0]` at `0x1400357cb`.
    pub(super) const STABILISER_CALLER: usize = 0x1_4003_57d1;
    /// `time_related_global_`, in seconds.
    pub(super) const FRAME_DT: usize = 0x1_403c_126c;

    pub(super) fn trampoline(framerate: u32) -> Trampoline {
        let mut trampoline = Trampoline::new();

        // cmp dword ptr [rsp], STABILISER_CALLER - the low half of the return address is enough
        // (the image is at 0x140000000), and it clobbers no register on the skipped path.
        trampoline.bytes(&[0x81, 0x3C, 0x24]);
        trampoline.bytes(&(STABILISER_CALLER as u32).to_le_bytes());
        // jne past the scaling: every other Body_AddTorque caller stays vanilla.
        trampoline.bytes(&[0x75, 0x1F]);

        // mov rax, FRAME_DT ; movss xmm0, [rax]
        trampoline.bytes(&[0x48, 0xB8]);
        trampoline.bytes(&(FRAME_DT as u64).to_le_bytes());
        trampoline.bytes(&[0xF3, 0x0F, 0x10, 0x00]);

        // mov eax, 50.0f ; movd xmm2, eax ; mulss xmm0, xmm2
        trampoline.bytes(&[0xB8]);
        trampoline.bytes(&framerate.to_le_bytes());
        trampoline.bytes(&[0x66, 0x0F, 0x6E, 0xD0, 0xF3, 0x0F, 0x59, 0xC2]);

        // mulss xmm1, xmm0
        trampoline.bytes(&[0xF3, 0x0F, 0x59, 0xC8]);

        // The stolen prologue, unchanged.
        trampoline.bytes(&EXPECTED);

        // Back into the body with the frame it expects (the stolen SUB RSP,0xC0 sets it).
        trampoline.relative_jump(CONTINUATION);
        trampoline
    }
}

#[cfg(feature = "1_151")]
mod v1_151 {
    use patchy::Trampoline;

    /// The case's `mov rax,[rdi]; mov rcx,rdi; call [rax+slot]` block.
    pub(super) const HOOK: usize = 0x1_4003_4469;
    pub(super) const STOLEN_SIZE: usize = 12;
    pub(super) const EXPECTED: [u8; STOLEN_SIZE] = [
        0x48, 0x8B, 0x07, // mov rax,[rdi]
        0x48, 0x8B, 0xCF, // mov rcx,rdi
        0xFF, 0x90, 0x98, 0x01, 0x00, 0x00, // call qword ptr [rax+0x198]
    ];
    /// The case's shared tail.
    pub(super) const CONTINUATION: usize = 0x1_4004_2f22;
    /// `time_related_global_`, in seconds.
    pub(super) const FRAME_DT: usize = 0x1_4039_1184;

    pub(super) fn trampoline(framerate: u32) -> Trampoline {
        let mut trampoline = Trampoline::new();

        // mov rax, FRAME_DT ; movss xmm0, [rax]
        trampoline.bytes(&[0x48, 0xB8]);
        trampoline.bytes(&(FRAME_DT as u64).to_le_bytes());
        trampoline.bytes(&[0xF3, 0x0F, 0x10, 0x00]);

        // mov eax, 50.0f ; movd xmm2, eax ; mulss xmm0, xmm2
        trampoline.bytes(&[0xB8]);
        trampoline.bytes(&framerate.to_le_bytes());
        trampoline.bytes(&[0x66, 0x0F, 0x6E, 0xD0, 0xF3, 0x0F, 0x59, 0xC2]);

        // mulss xmm1, xmm0
        trampoline.bytes(&[0xF3, 0x0F, 0x59, 0xC8]);

        // The stolen block, unchanged: mov rax,[rdi] ; mov rcx,rdi ; call [rax+slot]
        trampoline.bytes(&EXPECTED);

        trampoline.relative_jump(CONTINUATION);
        trampoline
    }
}

#[cfg(feature = "1_151")]
use v1_151 as imp;
#[cfg(not(feature = "1_151"))]
use v1_163 as imp;

/// Prepares the frame-rate-independent anti-tilt torque.
///
/// The hook is refused (and the game's own behaviour kept) if the code at the patch site is not
/// the expected block.
///
/// # Safety
///
/// The hook address must belong to the running game image. The caller must not install this
/// patch while another thread is executing the patched code.
pub unsafe fn patch_antitilt(session: &mut PatchSession) -> Result {
    if (imp::HOOK as *const [u8; imp::STOLEN_SIZE]).read_unaligned() != imp::EXPECTED {
        log::error!(
            "Anti-tilt: code at {:#x} is not what was expected, leaving it alone",
            imp::HOOK
        );
        return Ok(());
    }

    session.detour_trampoline(imp::HOOK, imp::STOLEN_SIZE, |_site| {
        Ok(imp::trampoline(VANILLA_FRAMERATE))
    })?;
    log::info!("Anti-tilt torque now scales with the frame time (unchanged at 50 fps)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trampoline_scales_only_the_stabiliser_call() {
        assert_eq!(f32::from_bits(VANILLA_FRAMERATE), 50.0);

        let base = 0x1_8000_0000usize;
        let code = imp::trampoline(VANILLA_FRAMERATE).build(base).unwrap();

        let mut expected = Vec::new();
        #[cfg(not(feature = "1_151"))]
        {
            // cmp dword ptr [rsp], STABILISER_CALLER ; jne +0x1F
            expected.extend_from_slice(&[0x81, 0x3C, 0x24]);
            expected.extend_from_slice(&(imp::STABILISER_CALLER as u32).to_le_bytes());
            expected.extend_from_slice(&[0x75, 0x1F]);
        }
        // mov rax, FRAME_DT ; movss xmm0, [rax]
        expected.extend_from_slice(&[0x48, 0xB8]);
        expected.extend_from_slice(&(imp::FRAME_DT as u64).to_le_bytes());
        expected.extend_from_slice(&[0xF3, 0x0F, 0x10, 0x00]);
        // mov eax, 50.0f ; movd xmm2, eax ; mulss xmm0, xmm2 ; mulss xmm1, xmm0
        expected.extend_from_slice(&[0xB8, 0x00, 0x00, 0x48, 0x42]);
        expected.extend_from_slice(&[0x66, 0x0F, 0x6E, 0xD0]);
        expected.extend_from_slice(&[0xF3, 0x0F, 0x59, 0xC2]);
        expected.extend_from_slice(&[0xF3, 0x0F, 0x59, 0xC8]);
        // The stolen block.
        expected.extend_from_slice(&imp::EXPECTED);
        // The jump back into the game.
        let jump_address = base + expected.len();
        expected.push(0xE9);
        let displacement = imp::CONTINUATION as i64 - (jump_address as i64 + 5);
        expected.extend_from_slice(&(displacement as i32).to_le_bytes());

        assert_eq!(code, expected);
    }

    #[cfg(not(feature = "1_151"))]
    #[test]
    fn the_skipped_branch_lands_on_the_replayed_prologue() {
        // `jne +0x1F` (2 bytes) must land exactly on the stolen `PUSH RDI`.
        let code = imp::trampoline(VANILLA_FRAMERATE)
            .build(0x1_8000_0000)
            .unwrap();
        let jne_at = 3 + 4; // cmp dword ptr [rsp], imm32
        assert_eq!(code[jne_at], 0x75);
        let target = jne_at + 2 + i8::from_le_bytes([code[jne_at + 1]]) as usize;
        assert_eq!(target, jne_at + 2 + 31);
        assert_eq!(&code[target..target + 2], &[0x40, 0x57]);
    }
}
