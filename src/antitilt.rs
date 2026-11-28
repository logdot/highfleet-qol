//! Makes the engine anti-tilt torque independent of the frame rate.
//!
//! `m_mdl_at` in `Libraries/OL.seria` drives the per-engine anti-tilt torque in `creatureLogic`'s `logic id 200` case.
//! The case runs once per frame and ends by calling `Body_AddTorque` on the hull Frame.
//! `Body_AddTorque` changes the hull's angular velocity immediately and is not multiplied by the frame `dt`, so the anti-tilt work done per second scales with the frame rate.
//! At a modified 100 fps the hull is pushed back to level twice as hard as at the vanilla 50 fps.
//!
//! Steam 1.163 hooks the **entry of `Body_AddTorque`** rather than the stabiliser case and scales the torque only for the stabiliser's own call.
//! That call is recognised by the return address of the `CALL [RAX+0x1A0]` at `0x1400357cb`, which is `0x1400357d1`.
//! The trampoline calls [`scale_torque`], which multiplies the torque by `dt * 50` (`1.0` at the vanilla 50 fps, `0.5` at 100 fps), replays the stolen entry and resumes the function body.
//! Hooking the torque entry rather than the case keeps the case's instructions free, so a mod that retargets the angle term there (`hf_wish_tilt`) composes with this factor instead of replacing it.
//!
//! Steam 1.151 hooks the case's `mov rax,[rdi]; mov rcx,rdi; call [rax+slot]` block instead, which no 1.151 mod retargets.
//!
//! The frame `dt` is the game clock at `time_related_global_`, which is `1 / CORE_FRAMERATE` times `CORE_TIMEWRAP`.
//! Using it also keeps the anti-tilt consistent if the time wrap changes.
//! Disabling `enable_antitilt_fps_fix` in `qol.json` leaves the game's own behaviour untouched and lets `hf_wish_tilt` apply the factor itself.

use patchy::{
    CallBridge, Condition, DetourSite, FlagPolicy, KeepReturn, PatchSession, Result,
    StackAlignment, Trampoline,
};

/// `50.0`, the vanilla `CORE_FRAMERATE` the anti-tilt behaviour is defined for.
const VANILLA_FRAMERATE: f32 = 50.0;

/// `MOVAPS XMM1, XMM0`: moves the bridge's result over the torque the replay expects.
const MOVE_RESULT_TO_TORQUE: [u8; 3] = [0x0F, 0x28, 0xC8];

#[cfg(not(feature = "1_151"))]
mod v1_163 {
    use super::*;

    /// `Body_AddTorque` entry: `PUSH RDI; SUB RSP,0xC0`.
    pub(super) const HOOK: usize = 0x1_4001_9be0;
    /// Size of those two instructions, which relocate unchanged into the trampoline.
    pub(super) const STOLEN_SIZE: usize = 9;
    /// `time_related_global_`, in seconds.
    pub(super) const FRAME_DT: usize = 0x1_403c_126c;
    /// `Body_AddTorque` is entered with the return address already pushed.
    pub(super) const STACK_ALIGNMENT: StackAlignment = StackAlignment::Offset8;

    /// The low half of the stabiliser's return address.
    ///
    /// `init` refuses to load the mod at any image base other than `0x140000000`, so comparing
    /// the low half is enough.
    const STABILISER_CALLER: u32 = 0x4003_57d1;

    /// `CMP dword ptr [RSP], STABILISER_CALLER`: only the stabiliser reaches the scaling.
    pub(super) const CALLER_TEST: Option<[u8; 7]> = Some({
        let caller = STABILISER_CALLER.to_le_bytes();
        [0x81, 0x3C, 0x24, caller[0], caller[1], caller[2], caller[3]]
    });
}

#[cfg(feature = "1_151")]
mod v1_151 {
    use super::*;

    /// The case's `mov rax,[rdi]; mov rcx,rdi; call [rax+0x198]` block.
    pub(super) const HOOK: usize = 0x1_4003_4469;
    /// Size of that block, which relocates unchanged into the trampoline.
    pub(super) const STOLEN_SIZE: usize = 12;
    /// `time_related_global_`, in seconds.
    pub(super) const FRAME_DT: usize = 0x1_4039_1184;
    /// The case runs inside the function body, where the prologue leaves `RSP` 16-byte aligned.
    pub(super) const STACK_ALIGNMENT: StackAlignment = StackAlignment::Aligned16;

    /// The case only runs for the stabiliser, so there is no caller to test.
    pub(super) const CALLER_TEST: Option<[u8; 7]> = None;
}

#[cfg(feature = "1_151")]
use v1_151 as imp;
#[cfg(not(feature = "1_151"))]
use v1_163 as imp;

/// Multiplies `torque` by the frame length relative to the vanilla one.
fn scaled_torque(torque: f32, frame_time: f32) -> f32 {
    torque * (frame_time * VANILLA_FRAMERATE)
}

/// Reads the frame length and returns the scaled torque in `XMM0`.
///
/// # Safety
///
/// The game must be running, because this reads `time_related_global_`.
unsafe extern "C" fn scale_torque(_: f32, torque: f32) -> f32 {
    // SAFETY: the running game owns this global, which holds the frame length in seconds.
    let frame_time = unsafe { (imp::FRAME_DT as *const f32).read() };

    scaled_torque(torque, frame_time)
}

/// Builds a trampoline that scales the torque, replays the stolen span and resumes the game.
fn trampoline(site: DetourSite) -> Result<Trampoline> {
    let mut trampoline = Trampoline::new();
    let scaled = trampoline.new_label();

    if let Some(caller_test) = imp::CALLER_TEST {
        trampoline.bytes(&caller_test);
        // Every other Body_AddTorque caller lands on the replay and stays vanilla.
        trampoline.jump_if(Condition::NotEqual, scaled);
    }

    // SAFETY: `XMM1` holds the torque, the bridge leaves its result in `XMM0` for the move
    // below, and `imp::STACK_ALIGNMENT` describes `RSP` at the site.
    unsafe {
        trampoline.call(CallBridge {
            callback: scale_torque as *const (),
            incoming_stack: imp::STACK_ALIGNMENT,
            flags: FlagPolicy::Preserve,
            keep_return: KeepReturn::Xmm0,
            before_call: &[],
            after_call: &[],
        });
    }
    trampoline.bytes(&MOVE_RESULT_TO_TORQUE);
    trampoline.bind(scaled)?;

    // SAFETY: both stolen spans hold whole instructions without relative or RIP-relative
    // operands, so they run unchanged in the trampoline.
    unsafe { site.replay_verbatim(&mut trampoline) };
    site.resume(&mut trampoline);
    Ok(trampoline)
}

/// Prepares the frame-rate-independent anti-tilt torque.
///
/// # Safety
///
/// The hook address must belong to the running game image.
/// The caller must not install this patch while another thread is executing the patched code.
pub unsafe fn patch_antitilt(session: &mut PatchSession) -> Result {
    session.detour_trampoline(imp::HOOK, imp::STOLEN_SIZE, trampoline)?;
    log::info!("Anti-tilt torque now scales with the frame time (unchanged at 50 fps)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_is_identity_at_the_vanilla_frame_rate() {
        let vanilla = 1.0 / VANILLA_FRAMERATE;

        assert_eq!(scaled_torque(4.0, vanilla), 4.0);
        assert_eq!(scaled_torque(4.0, vanilla / 2.0), 2.0);
    }
}
