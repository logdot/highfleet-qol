use crate::patchy::{Patch, ReturnType};

static mut MIN_ZOOM: u32 = 3;
static mut MAX_ZOOM: u32 = 3;
static mut ZOOM_LEVELS: Vec<f32> = Vec::new();

#[cfg(feature = "1_151")]
static MIN_ZOOM_ADDR: usize = 0x143942538;
#[cfg(feature = "1_151")]
static MAX_ZOOM_ADDR: usize = 0x140391160;
#[cfg(feature = "1_151")]
static ZOOM_LEVEL_ADDR: usize = 0x14039115c;

#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
static MIN_ZOOM_ADDR: usize = 0x143a119a4;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
static MAX_ZOOM_ADDR: usize = 0x1403c11d0;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
static ZOOM_LEVEL_ADDR: usize = 0x1403c11cc;

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

    let p = Patch::patch_call(address, set_zoom_level as *const (), override_count, false, ReturnType::None);
    std::mem::forget(p);
}

pub unsafe fn patch_levels(zoom_levels: Vec<f32>) {
    ZOOM_LEVELS = zoom_levels;

    let address;
    if cfg!(feature = "1_151") {
        address = 0x140249371;
    } else if cfg!(feature = "1_163") {
        address = 0x14026b03f;
    } else {
        // Default to 1.163
        address = 0x14026b03f;
    }

    let p = Patch::patch_call(address, calc_zoom_value as *const (), 5, false, ReturnType::Xmm0);
    std::mem::forget(p);
}

unsafe extern "C" fn set_zoom_level() {
    let max = MAX_ZOOM;
    let min = MIN_ZOOM;

    let max_level = MAX_ZOOM_ADDR as *mut u32;
    *max_level = max;

    let min_level = MIN_ZOOM_ADDR as *mut u32;
    *min_level = min;
}

#[cfg(feature = "1_151")]
static IS_IN_ARCADE: usize = 0x147eed995;
#[cfg(feature = "1_151")]
static REAL_CALC_ZOOM: usize = 0x14022da90;

#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
static IS_IN_ARCADE: usize = 0x147fc6fb7;
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
static REAL_CALC_ZOOM: usize = 0x14024f170;

#[allow(static_mut_refs)]
unsafe extern "C" fn calc_zoom_value() -> f32 {
    let is_in_arcade = IS_IN_ARCADE as *const bool;

    if !*is_in_arcade {
        // Call original function if both booleans are false
        let func: extern "C" fn() -> f32 = std::mem::transmute(REAL_CALC_ZOOM as *const ());
        return func();
    }

    let zoom_value = ZOOM_LEVEL_ADDR as *const u32;

    if *zoom_value > ZOOM_LEVELS.len() as u32 {
        return 1.0;
    }
    ZOOM_LEVELS[*zoom_value as usize]
}
