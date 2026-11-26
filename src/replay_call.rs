//! Hooks that keep the overwritten instructions and then call into the mod.

use patchy::{CallBridge, PatchSession, Result, Trampoline};

/// Stages a detour that replays the `size` overwritten bytes, calls `bridge`, and resumes after them.
///
/// [`PatchSession::patch_call`] resumes without replaying, so these sites need a custom trampoline.
///
/// # Safety
///
/// The site must satisfy [`PatchSession::detour_trampoline_with`].
/// The overwritten instructions must be valid when replayed verbatim at another address.
/// `bridge.incoming_stack` must describe `RSP` after the replayed instructions.
pub unsafe fn replay_then_call(
    session: &mut PatchSession,
    address: usize,
    size: usize,
    bridge: CallBridge<'_>,
) -> Result {
    session.detour_trampoline_with(address, size, |site| {
        let mut code = Trampoline::new();
        site.replay_verbatim(&mut code);
        code.call(bridge);
        site.resume(&mut code);
        Ok(code)
    })?;
    Ok(())
}
