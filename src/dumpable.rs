use std::ffi::c_void;
use std::arch::asm;

use windows::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS};

use crate::patchy::Patch;

pub unsafe fn dumpable() {
    let address;
    if cfg!(feature = "1_151") {
        address = 0x1400240c0;
    } else if cfg!(feature = "1_163") {
        address = 0x1400256e0;
    } else {
        // Default to 1.163
        address = 0x1400256e0;
    }

    let mut old_protect = PAGE_PROTECTION_FLAGS(0);

    VirtualProtect(
        address as *mut c_void,
        0x100,
        PAGE_EXECUTE_READWRITE,
        &mut old_protect as *mut _,
    ).unwrap();

    let p = Patch::patch_call(address, set_dumpable as *const (), 6, true);
    std::mem::forget(p);

    VirtualProtect(
        address as *mut c_void,
        0x100,
        old_protect,
        &mut old_protect as *mut _,
    ).unwrap();
}

#[no_mangle]
#[cfg(feature = "1_151")]
unsafe extern "C" fn set_dumpable() {
    asm! {
        "mov byte ptr [rsi + 0x8e6], 0",
        out("rsi") _,
    }
}

#[no_mangle]
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
unsafe extern "C" fn set_dumpable() {
    asm! {
        "mov byte ptr [rsi + 0x91E], 0",
        out("rsi") _,
    }
}
