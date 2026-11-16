//! Optional suppression of enemy pre-fire gun lines in Steam 1.163.
//!
//! In BattleHud_DrawCombatMarkers (0x14029c5c0), these two calls draw
//! group-0 and group-1 gun warnings. RSI is the current creature and XMM8 is
//! zero. The immediately following native code uses alignment at +0xad4 to
//! distinguish hostile ships for the warning sound. Preserve that code.

use patchy::{Condition, PatchError, PatchSession, Result, Trampoline};
use std::mem::{size_of, MaybeUninit};
use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD,
};

const DRAW_WORLD_LINE: usize = 0x140248ba0;
const IMAGE_BASE: usize = 0x140000000;
// BattleHud_DrawCombatMarkers calls playSFXSound_ with "alarm_small_09" here.
const SOUND_CALL: usize = 0x14029e119;
const SOUND_CALL_ORIGINAL: [u8; 5] = [0xe8, 0xc2, 0x4f, 0x04, 0x00];
const SITES: [(usize, [u8; 5]); 2] = [
    (0x14029d664, [0xe8, 0x37, 0xb5, 0xfa, 0xff]),
    (0x14029d956, [0xe8, 0x45, 0xb2, 0xfa, 0xff]),
];

pub unsafe fn prepare(session: &mut PatchSession) -> Result {
    for (address, expected) in SITES {
        if !matches_executable_bytes(address, &expected) {
            return Err(PatchError::SourceChanged { address });
        }
    }
    for (address, _) in SITES {
        let mut trampoline = Trampoline::new();
        let skip_draw = trampoline.new_label();
        // COMISS XMM8,[RSI+0xad4]: zero >= alignment means hostile.
        trampoline.bytes(&[0x44, 0x0f, 0x2f, 0x86, 0xd4, 0x0a, 0x00, 0x00]);
        trampoline.jump_if(Condition::AboveOrEqual, skip_draw);
        trampoline.absolute_call(DRAW_WORLD_LINE as *const ());
        trampoline.bind(skip_draw)?;
        trampoline.relative_jump(address + 5);
        // SAFETY: The exact original call is checked above; RSI and XMM8 have
        // the meanings established by the enclosing 1.163 render loop. The
        // original argument registers and stack are untouched before the call.
        unsafe { session.detour_trampoline(address, 5, trampoline)? };
    }
    log::info!("Prepared enemy pre-fire laser warning suppression");
    Ok(())
}

/// Suppress the enemy pre-fire alarm without changing the warning state.
pub unsafe fn prepare_sound_suppression(session: &mut PatchSession) -> Result {
    if !matches_executable_bytes(SOUND_CALL, &SOUND_CALL_ORIGINAL) {
        return Err(PatchError::SourceChanged {
            address: SOUND_CALL,
        });
    }
    // SAFETY: The verified instruction is a complete five-byte sound call.
    // Its return value is unused; the original warning-state update follows it.
    unsafe { session.overwrite(SOUND_CALL, &[0x90; 5])? };
    log::info!("Prepared enemy pre-fire sound warning suppression");
    Ok(())
}

fn matches_executable_bytes(address: usize, expected: &[u8]) -> bool {
    let mut information = MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
    // SAFETY: The output buffer is aligned and sized for VirtualQuery.
    let queried = unsafe {
        VirtualQuery(
            address as *const _,
            information.as_mut_ptr(),
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if queried != size_of::<MEMORY_BASIC_INFORMATION>() {
        return false;
    }
    // SAFETY: VirtualQuery reported a complete initialized structure.
    let information = unsafe { information.assume_init() };
    if information.State != MEM_COMMIT
        || information.Protect & PAGE_GUARD != 0
        || !matches!(
            information.Protect,
            PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
        )
        || information.AllocationBase as usize != IMAGE_BASE
    {
        return false;
    }
    let offset = address.saturating_sub(information.BaseAddress as usize);
    if expected.len() > information.RegionSize.saturating_sub(offset) {
        return false;
    }
    // SAFETY: The range is committed readable code in the main image and
    // Modloader prepares patches before gameplay starts.
    unsafe { std::slice::from_raw_parts(address as *const u8, expected.len()) == expected }
}
