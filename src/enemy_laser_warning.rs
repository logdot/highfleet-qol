//! Optional suppression of pre-fire gun warning lines in Steam 1.163.
//!
//! In BattleHud_DrawCombatMarkers (0x14029c5c0), these two calls draw the
//! group-0 and group-1 gun warnings for both sides.
//! The immediately following native code updates the warning sound state
//! independently; preserve that code.

use patchy::{PatchSession, Result};

// BattleHud_DrawCombatMarkers calls playSFXSound_ with "alarm_small_09" here.
const SOUND_CALL: usize = 0x14029e119;
const WARNING_LINE_CALLS: [usize; 2] = [0x14029d664, 0x14029d956];

pub unsafe fn prepare(session: &mut PatchSession) -> Result {
    for address in WARNING_LINE_CALLS {
        // SAFETY: The instruction is a complete five-byte draw call.
        // Its return value is unused.
        unsafe { session.overwrite(address, &[0x90; 5])? };
    }

    log::info!("Prepared pre-fire laser warning suppression for both sides");
    Ok(())
}

/// Suppress the enemy pre-fire alarm without changing the warning state.
pub unsafe fn prepare_sound_suppression(session: &mut PatchSession) -> Result {
    // SAFETY: The instruction is a complete five-byte sound call.
    // Its return value is unused; the original warning-state update follows it.
    unsafe { session.overwrite(SOUND_CALL, &[0x90; 5])? };

    log::info!("Prepared enemy pre-fire sound warning suppression");
    Ok(())
}
