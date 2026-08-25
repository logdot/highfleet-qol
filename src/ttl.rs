use patchy::{PatchSession, Result};

pub unsafe fn patch_ttl(session: &mut PatchSession) -> Result {
    let address;
    if cfg!(feature = "1_151") {
        address = 0x140050372;
    } else if cfg!(feature = "1_163") {
        address = 0x140052af2;
    } else {
        // Default to 1.163
        address = 0x140052af2;
    }

    let data = [0x90u8; 4]; // NOP instructions
    session.overwrite(address, &data)?;
    Ok(())
}
