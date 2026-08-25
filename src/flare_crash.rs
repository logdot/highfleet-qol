//! Restores HighFleet 1.151's nullable missile-fuze link check in version 1.163.

use std::slice;

use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use patchy::Patch;

const HOOK_RVA: usize = 0x36bb5;
const NON_NULL_RESUME_RVA: usize = 0x36bbb;
const NULL_EXIT_RVA: usize = 0x36ca2;
const ORIGINAL_BYTES: [u8; 6] = [0x48, 0x8b, 0xd8, 0x48, 0x8b, 0xce];

/// Installs the nullable missile-fuze link check required by HighFleet 1.163.
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
pub unsafe fn patch_flare_crash() {
    let module_base = match GetModuleHandleW(None) {
        Ok(module) => module.0 as usize,
        Err(error) => {
            log::error!("flare_crash_fix: unable to locate the game executable: {error}");
            return;
        }
    };
    let hook_address = module_base + HOOK_RVA;
    let found_bytes = slice::from_raw_parts(hook_address as *const u8, ORIGINAL_BYTES.len());

    if found_bytes != ORIGINAL_BYTES {
        log::error!(
            "flare_crash_fix: unexpected bytes at {hook_address:#x}; expected {:02x?}, found {:02x?}",
            ORIGINAL_BYTES,
            found_bytes
        );
        return;
    }

    let trampoline = build_trampoline(module_base);
    let patch = Patch::detour(hook_address, ORIGINAL_BYTES.len(), &trampoline);
    std::mem::forget(patch);

    log::info!("Flare crash fix enabled");
}

/// Reports that the 1.163-only fix is unnecessary on HighFleet 1.151.
#[cfg(feature = "1_151")]
pub unsafe fn patch_flare_crash() {
    log::info!("Flare crash fix is not required on HighFleet 1.151");
}

fn build_trampoline(module_base: usize) -> Vec<u8> {
    let mut trampoline = Vec::with_capacity(39);

    // TEST RAX, RAX; JNZ non_null (skip the 14-byte absolute null jump).
    trampoline.extend_from_slice(&[0x48, 0x85, 0xc0, 0x75, 0x0e]);
    push_absolute_jump(&mut trampoline, module_base + NULL_EXIT_RVA);

    // Replay the instructions overwritten at the hook point.
    trampoline.extend_from_slice(&ORIGINAL_BYTES);
    push_absolute_jump(&mut trampoline, module_base + NON_NULL_RESUME_RVA);

    trampoline
}

fn push_absolute_jump(code: &mut Vec<u8>, destination: usize) {
    // JMP qword ptr [RIP]; the destination pointer immediately follows.
    code.extend_from_slice(&[0xff, 0x25, 0x00, 0x00, 0x00, 0x00]);
    code.extend_from_slice(&destination.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trampoline_branches_to_the_expected_runtime_addresses() {
        let module_base = 0x140000000;
        let trampoline = build_trampoline(module_base);

        assert_eq!(&trampoline[..5], &[0x48, 0x85, 0xc0, 0x75, 0x0e]);
        assert_eq!(
            usize::from_le_bytes(trampoline[11..19].try_into().unwrap()),
            module_base + NULL_EXIT_RVA
        );
        assert_eq!(&trampoline[19..25], &ORIGINAL_BYTES);
        assert_eq!(
            usize::from_le_bytes(trampoline[31..39].try_into().unwrap()),
            module_base + NON_NULL_RESUME_RVA
        );
        assert_eq!(trampoline.len(), 39);
    }
}
