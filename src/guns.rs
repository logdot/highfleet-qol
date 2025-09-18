#[cfg(feature = "1_151")]
use crate::patchy::Patch;

#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_blocking() {
    let address: usize = 0x14003314d;
    let size: usize = 6;

    let data = vec![0x90; size]; // NOP instructions
    let p = Patch::overwrite(address, &data);
    std::mem::forget(p);
}

// Gun arcs are already disabled in 1.163
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
pub unsafe fn patch_sector_blocking() {}
