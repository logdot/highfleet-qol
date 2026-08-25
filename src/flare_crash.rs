//! Restores HighFleet 1.151's nullable missile-fuze link check in version 1.163.

use std::slice;

use patchy::{Patch, PatchError, ProcessModule, Trampoline};

const HOOK_RVA: usize = 0x36bb5;
const NON_NULL_RESUME_RVA: usize = 0x36bbb;
const NULL_EXIT_RVA: usize = 0x36ca2;
const ORIGINAL_BYTES: [u8; 6] = [0x48, 0x8b, 0xd8, 0x48, 0x8b, 0xce];

/// Installs the nullable missile-fuze link check required by HighFleet 1.163.
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
pub unsafe fn patch_flare_crash() {
    let module = match ProcessModule::main() {
        Ok(module) => module,
        Err(error) => {
            log::error!("flare_crash_fix: unable to locate the game executable: {error}");
            return;
        }
    };
    let hook_address = match module.resolve_rva(HOOK_RVA) {
        Ok(address) => address,
        Err(error) => {
            log::error!("flare_crash_fix: unable to resolve hook address: {error}");
            return;
        }
    };
    let found_bytes = slice::from_raw_parts(hook_address as *const u8, ORIGINAL_BYTES.len());

    if found_bytes != ORIGINAL_BYTES {
        log::error!(
            "flare_crash_fix: unexpected bytes at {hook_address:#x}; expected {:02x?}, found {:02x?}",
            ORIGINAL_BYTES,
            found_bytes
        );
        return;
    }

    let trampoline = match build_trampoline(module) {
        Ok(trampoline) => trampoline,
        Err(error) => {
            log::error!("flare_crash_fix: unable to build trampoline: {error}");
            return;
        }
    };
    Patch::detour_trampoline(hook_address, ORIGINAL_BYTES.len(), trampoline);

    log::info!("Flare crash fix enabled");
}

/// Reports that the 1.163-only fix is unnecessary on HighFleet 1.151.
#[cfg(feature = "1_151")]
pub unsafe fn patch_flare_crash() {
    log::info!("Flare crash fix is not required on HighFleet 1.151");
}

fn build_trampoline(module: ProcessModule) -> Result<Trampoline, PatchError> {
    let mut trampoline = Trampoline::new();
    let non_null = trampoline.new_label();

    trampoline.bytes(&[0x48, 0x85, 0xc0]); // TEST RAX, RAX
    trampoline.jump_if_not_zero(non_null);
    trampoline.absolute_jump(module.resolve_rva(NULL_EXIT_RVA)?);

    // Replay the instructions overwritten at the hook point.
    trampoline.bind(non_null)?;
    trampoline.bytes(&ORIGINAL_BYTES);
    trampoline.absolute_jump(module.resolve_rva(NON_NULL_RESUME_RVA)?);

    Ok(trampoline)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trampoline_branches_to_the_expected_runtime_addresses() {
        let module_base = 0x140000000;
        let trampoline = build_trampoline(ProcessModule::from_base(module_base)).unwrap();
        let trampoline = trampoline.build(0x180000000).unwrap();

        assert_eq!(&trampoline[..3], &[0x48, 0x85, 0xc0]);
        assert_eq!(&trampoline[3..9], &[0x0f, 0x85, 0x0e, 0x00, 0x00, 0x00]);
        assert_eq!(
            usize::from_le_bytes(trampoline[15..23].try_into().unwrap()),
            module_base + NULL_EXIT_RVA
        );
        assert_eq!(&trampoline[23..29], &ORIGINAL_BYTES);
        assert_eq!(
            usize::from_le_bytes(trampoline[35..43].try_into().unwrap()),
            module_base + NON_NULL_RESUME_RVA
        );
        assert_eq!(trampoline.len(), 43);
    }
}
