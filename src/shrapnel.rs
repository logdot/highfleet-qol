//! Prototype: shrink logic-150 fragment geometry on Steam 1.163.

use patchy::{PatchError, PatchSession, ProcessModule, Result};

const SIZE_LOAD_RVA: usize = 0x33e88;
const SMALL_SIZE_RVA: usize = 0x35475c;
const ORIGINAL_BYTES: [u8; 8] = [0xf3, 0x0f, 0x10, 0x3d, 0x90, 0xe9, 0x31, 0x00];

pub unsafe fn patch_shrapnel_size(session: &mut PatchSession) -> Result {
    let module = ProcessModule::main()?;
    let address = module.resolve_rva(SIZE_LOAD_RVA)?;
    let size_address = module.resolve_rva(SMALL_SIZE_RVA)?;
    let found = std::slice::from_raw_parts(address as *const u8, ORIGINAL_BYTES.len());
    if found != ORIGINAL_BYTES {
        return Err(PatchError::SourceChanged { address });
    }
    if (size_address as *const f32).read_unaligned() != 1.25 {
        return Err(PatchError::SourceChanged {
            address: size_address,
        });
    }

    // XMM7 supplies both dimensions to CreateShell_ for every logic-150 fragment.
    // Redirect this load to the existing 1.25 constant; do not change shared 20.0 data.
    let displacement = (SMALL_SIZE_RVA as i32) - (SIZE_LOAD_RVA + ORIGINAL_BYTES.len()) as i32;
    let mut replacement = ORIGINAL_BYTES;
    replacement[4..].copy_from_slice(&displacement.to_le_bytes());
    session.overwrite(address, &replacement)?;
    log::info!("Prepared prototype logic-150 fragment size patch (20 x 20 -> 1.25 x 1.25)");
    Ok(())
}
