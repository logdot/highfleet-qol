use std::ffi::c_void;

use windows::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS};

use crate::patchy::Patch;

static mut MIN_ZOOM: u32 = 3;
static mut MAX_ZOOM: u32 = 3;

#[cfg(feature = "1_151")]
static MIN_ZOOM_ADDR: usize = 0x143942538;
#[cfg(feature = "1_151")]
static MAX_ZOOM_ADDR: usize = 0x140391160;

#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
static MIN_ZOOM_ADDR: usize = 0x143a119a4;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
static MAX_ZOOM_ADDR: usize = 0x1403c11d0;

pub unsafe fn patch_zoom(min_zoom: u32, max_zoom: u32) {
    MAX_ZOOM = max_zoom;
    MIN_ZOOM = min_zoom;

    let address;
    if cfg!(feature = "1_151") {
        address = 0x1401adf72;
    } else if cfg!(feature = "1_163") {
        address = 0x1402C31C9;
    } else {
        // Default to 1.163
        address = 0x1402C31C9;
    }

    // Default to 1.163
    let mut override_count = 14;
    if cfg!(feature = "1_151") {
        override_count = 20;
    }

    let mut old_protect = PAGE_PROTECTION_FLAGS(0);

    VirtualProtect(
        address as *mut c_void,
        0x100,
        PAGE_EXECUTE_READWRITE,
        &mut old_protect as *mut _,
    ).unwrap();

    let p = Patch::patch_call(address, set_zoom_level as *const (), override_count, false);
    std::mem::forget(p);

    VirtualProtect(
        address as *mut c_void,
        0x100,
        old_protect,
        &mut old_protect as *mut _,
    ).unwrap();
}

unsafe extern "C" fn set_zoom_level() {
    let max = MAX_ZOOM;
    let min = MIN_ZOOM;

    let max_level = MAX_ZOOM_ADDR as *mut u32;
    *max_level = max;

    let min_level = MIN_ZOOM_ADDR as *mut u32;
    *min_level = min;
}
