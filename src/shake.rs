use crate::patchy::Patch;

pub unsafe fn patch_shake() {
    let address;
    if cfg!(feature = "1_151") {
        address = 0x1403285e0;
    } else if cfg!(feature = "1_163") {
        address = 0x140354758;
    } else {
        // Default to 1.163
        address = 0x140354758;
    }

    // Hex representation of float 1.0
    let data = [0x00, 0x00, 0x80, 0x3F];

    let p = Patch::overwrite(address, &data);
    std::mem::forget(p);
}
