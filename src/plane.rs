struct TLL {}

#[repr(C)]
struct Loadout {
    // Pointer to a null-terminated string
    name: *mut u8,
    padding: [u8; 8],
    length: u64,
    max_length: u64,
    amount: u32,
    padding2: [u8; 4],
}

impl Loadout {
    fn new(name: &str, amount: u32) -> Self {
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0); // Null terminator

        let name_ptr = Box::into_raw(name_bytes.into_boxed_slice()) as *mut u8;

        // When first initialized length and max_length is 0
        Loadout {
            name: name_ptr,
            padding: [0; 8],
            length: 0,
            max_length: 0,
            amount,
            padding2: [0; 4],
        }
    }
}

pub unsafe fn patch_planes() {
    let address;
    if cfg!(feature = "1_151") {
        address = 0x14019fb09;
    } else if cfg!(feature = "1_163") {
        address = 0x0;
    } else {
        // Default to 1.163
        address = 0x0;
    }

    let p = crate::patchy::Patch::patch_call(address, patch_add_plane as *const (), 5, false, crate::patchy::ReturnType::Rax);
    std::mem::forget(p);
}

fn add_plane() -> extern "C" fn(*mut TLL, *mut TLL, *mut Loadout) -> *mut TLL {
	unsafe { std::mem::transmute(0x1401889a0_u64) }
}

extern "C" fn patch_add_plane(global_tll: *mut TLL, local_tll: *mut TLL, loadout: *mut Loadout) -> *mut TLL {
    add_plane()(global_tll, local_tll, loadout);

    let mut new_loadout = Loadout::new("CRAFT_MB110", 0);
    add_plane()(global_tll, local_tll, &mut new_loadout as *mut Loadout)
}
