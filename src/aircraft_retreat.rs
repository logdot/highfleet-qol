//! Opt-in 1.151 aircraft-vs-aircraft retreat prototype.
//!
//! Native call-site replacements retain the Windows x64 ABI and the parent screen
//! frame. The Ghidra screen continuations are not callable functions.

use std::{
    ffi::{c_char, c_void},
    mem::transmute,
    ptr, slice,
    sync::atomic::{AtomicBool, Ordering},
};

use highfleet::general::EscadraString;
use patchy::{relative_offset, PatchError, PatchSession, ProcessModule, Result};
use windows::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{
        VirtualAlloc, VirtualFree, VirtualProtect, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
        PAGE_EXECUTE_READ, PAGE_PROTECTION_FLAGS, PAGE_READWRITE,
    },
    Threading::GetCurrentProcess,
};

const SETUP: usize = 0x1402a6740;
const CLEANUP: usize = 0x1402a7190;
const COMPARE: usize = 0x140004590;
const CHOOSE_STATE: usize = 0x14000edb0;
const ACTION: &std::ffi::CStr = c"QOL_AIR_RETREAT";

const HOOKS: [(usize, [u8; 5]); 3] = [
    (0x14029f8fb, [0xe8, 0x40, 0x6e, 0x00, 0x00]),
    (0x14029f8df, [0xe8, 0xac, 0x78, 0x00, 0x00]),
    (0x140128e1b, [0xe8, 0x70, 0xb7, 0xed, 0xff]),
];

static ENCOUNTER_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) unsafe fn patch_aircraft_retreat(session: &mut PatchSession) -> Result<CallThunks> {
    if ProcessModule::main()?.base_address() != 0x140000000 {
        return Err(PatchError::ModuleLookup(
            "aircraft retreat prototype requires the native 1.151 image base".into(),
        ));
    }
    for (address, expected) in HOOKS {
        let found = slice::from_raw_parts(address as *const u8, expected.len());
        if found != expected {
            log::error!(
                "aircraft_retreat: refusing prototype; unexpected bytes at {address:#x}: {found:02x?}"
            );
            return Err(PatchError::SourceChanged { address });
        }
    }

    let replacements = [
        setup_encounter as *const (),
        cleanup_encounter as *const (),
        compare_retreat_action as *const (),
    ];
    // patchy-rs 0.1.0 only installs JMP detours. Keep a CALL at each native
    // source so unwinding and hardware shadow-stack returns remain valid.
    let thunks = CallThunks::allocate()?;
    for (index, replacement) in replacements.into_iter().enumerate() {
        let destination = thunks.address.cast::<u8>().add(index * 16);
        let mut code = [0; 14];
        code[..6].copy_from_slice(&[0xff, 0x25, 0, 0, 0, 0]);
        code[6..].copy_from_slice(&(replacement as usize).to_le_bytes());
        ptr::copy_nonoverlapping(code.as_ptr(), destination, code.len());
    }
    let mut old = PAGE_PROTECTION_FLAGS::default();
    VirtualProtect(thunks.address, 4096, PAGE_EXECUTE_READ, &mut old)
        .map_err(|error| PatchError::Mapping(error.to_string()))?;
    FlushInstructionCache(GetCurrentProcess(), Some(thunks.address), 4096).map_err(|error| {
        PatchError::InstructionCache {
            address: thunks.address as usize,
            error: error.to_string(),
        }
    })?;
    for (index, (address, _)) in HOOKS.into_iter().enumerate() {
        let mut call = [0xe8, 0, 0, 0, 0];
        call[1..].copy_from_slice(
            &relative_offset(address + 5, thunks.address as usize + index * 16)?.to_le_bytes(),
        );
        session.overwrite(address, &call)?;
    }
    log::warn!("Prepared experimental aircraft retreat for Steam 1.151; campaign disengagement is unverified");
    Ok(thunks)
}

/// Owned during preparation, then retained permanently before installation is attempted.
/// Dropping this before installation releases executable memory without publishing hooks.
pub(crate) struct CallThunks {
    address: *mut c_void,
}

impl CallThunks {
    unsafe fn allocate() -> Result<Self> {
        let anchor = HOOKS[0].0 & !0xffff;
        // Windows reservations use 64-KiB granularity. All three sources lie
        // close together; validate their individual rel32 ranges before use.
        for distance in (0x10000..0x70000000).step_by(0x10000) {
            for candidate in [anchor.checked_sub(distance), anchor.checked_add(distance)]
                .into_iter()
                .flatten()
            {
                if HOOKS.iter().enumerate().any(|(index, (source, _))| {
                    relative_offset(source + 5, candidate + index * 16).is_err()
                }) {
                    continue;
                }
                let address = VirtualAlloc(
                    Some(candidate as *const c_void),
                    4096,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                );
                if !address.is_null() {
                    return Ok(Self { address });
                }
            }
        }
        Err(PatchError::NoMemoryCave {
            hook: HOOKS[0].0,
            last_error: Some("unable to reserve the aircraft retreat CALL thunks".into()),
        })
    }
}

impl Drop for CallThunks {
    fn drop(&mut self) {
        if let Err(error) = unsafe { VirtualFree(self.address, 0, MEM_RELEASE) } {
            log::error!("aircraft_retreat: unable to release unpublished thunks: {error}");
        }
    }
}

unsafe fn read<T: Copy>(object: *const u8, offset: usize) -> T {
    ptr::read_unaligned(object.add(offset).cast())
}

unsafe fn global<T: Copy>(address: usize) -> T {
    ptr::read_unaligned(address as *const T)
}

unsafe fn virtual_address(object: *mut u8, offset: usize) -> usize {
    read(global::<*const u8>(object as usize), offset)
}

unsafe extern "C" fn setup_encounter() {
    ENCOUNTER_ACTIVE.store(false, Ordering::Relaxed);
    let original: unsafe extern "C" fn() = transmute(SETUP);
    original();

    let get_profile: unsafe extern "C" fn() -> *const u8 = transmute(0x14026ff50usize);
    let profile = get_profile();
    if profile.is_null() || !read::<*const u8>(profile, 0x60).is_null() {
        return;
    }
    let sentinel = global::<*const u8>(0x147eedd78);
    if sentinel.is_null() || read::<*const u8>(sentinel, 0) == sentinel {
        return;
    }
    if create_button() {
        ENCOUNTER_ACTIVE.store(true, Ordering::Relaxed);
        log::info!("aircraft_retreat: created air-vs-air retreat button");
    }
}

unsafe extern "C" fn cleanup_encounter() {
    // No GUI pointer is retained: the native screen owns and destroys the button.
    ENCOUNTER_ACTIVE.store(false, Ordering::Relaxed);
    let original: unsafe extern "C" fn() = transmute(CLEANUP);
    original();
}

unsafe fn create_button() -> bool {
    let root = global::<*mut u8>(0x147eed960);
    if root.is_null() {
        log::error!("aircraft_retreat: no native GUI root");
        return false;
    }
    let allocate: unsafe extern "C" fn(usize) -> *mut u8 = transmute(0x140309078usize);
    let construct: unsafe extern "C" fn(*mut u8, u8) -> *mut u8 = transmute(0x140121c30usize);
    let memory = allocate(0xe38);
    if memory.is_null() {
        log::error!("aircraft_retreat: native GUI allocation failed");
        return false;
    }
    let gui = construct(memory, 0);
    let mut label = EscadraString::new();
    let localize: unsafe extern "C" fn(*mut EscadraString, *const c_char) -> *mut EscadraString =
        transmute(0x14019e7b0usize);
    let c_str: unsafe extern "C" fn(*const EscadraString) -> *const c_char =
        transmute(0x140022a00usize);
    let destroy_string: unsafe extern "C" fn(*mut EscadraString) = transmute(0x140006830usize);
    localize(&mut label, c"RETREAT_BUTTON".as_ptr());

    // Exact argument slots/textures/font from the strike button's native CALL.
    let initialize: unsafe extern "C" fn(
        *mut u8,
        *const c_char,
        f32,
        f32,
        f32,
        f32,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        f32,
    ) = transmute(virtual_address(gui, 0x2b8));
    initialize(
        gui,
        ACTION.as_ptr(),
        0.0,
        global::<i32>(0x14394293c) as f32 * global::<f32>(0x140326694) - global::<f32>(0x1403286f0),
        global(0x14032671c),
        global(0x140326714),
        c_str(&label),
        0x14033f3d8 as *const c_char,
        0x140339d30 as *const c_char,
        0x140339d30 as *const c_char,
        0x140330640 as *const c_char,
        global(0x1403263e8),
    );
    destroy_string(&mut label);
    let load: unsafe extern "C" fn(*mut u8) = transmute(virtual_address(gui, 0x88));
    load(gui);
    ptr::write_unaligned(gui.add(0xa54).cast::<i32>(), -1);

    let begin = global::<*const u8>(0x14391dbe8);
    let end = global::<*const u8>(0x14391dbf0);
    let actor = if !begin.is_null() && begin != end {
        let first = read::<*const u8>(begin, 0);
        if first.is_null() {
            ptr::null_mut()
        } else {
            read::<*mut u8>(first, 8)
        }
    } else {
        ptr::null_mut()
    };
    let add: unsafe extern "C" fn(*mut u8, *mut u8, *mut u8, u8) =
        transmute(virtual_address(root, 0x50));
    add(root, gui, actor, 0);
    true
}

unsafe extern "C" fn compare_retreat_action(
    action: *const c_void,
    expected: *const c_char,
) -> bool {
    let compare: unsafe extern "C" fn(*const c_void, *const c_char) -> bool = transmute(COMPARE);
    if !action.is_null() && compare(action, ACTION.as_ptr()) {
        if ENCOUNTER_ACTIVE.load(Ordering::Relaxed)
            && global::<u32>(0x147eed9a0) == 2
            && global::<u8>(0x147eed9b6) != 0
        {
            withdraw_player_aircraft();
        }
        // This private action must never reach the original AA_RETREAT branch.
        return false;
    }
    compare(action, expected)
}

unsafe fn withdraw_player_aircraft() {
    // FUN_1400e21a0 sorts positive/player strategic groups into 1439443c0.
    // Native setup spawns those groups into 147eedd78, independent of attacker.
    let sentinel = global::<*const u8>(0x147eedd78);
    if sentinel.is_null() {
        return;
    }
    let choose_state: unsafe extern "C" fn(*mut u8, i32) = transmute(CHOOSE_STATE);
    let mut entry = read::<*const u8>(sentinel, 0);
    let mut withdrawn = 0;
    // Bound traversal so a corrupt native list cannot hang the game in our hook.
    for _ in 0..4096 {
        if entry.is_null() || entry == sentinel {
            break;
        }
        let next = read::<*const u8>(entry, 0);
        let aircraft = read::<*const u8>(entry, 0x18);
        if !aircraft.is_null()
            && read::<i32>(aircraft, 0x48) != 0
            && read::<i32>(aircraft, 0x3b4) == 500
        {
            let logic = read::<*mut u8>(aircraft, 0x9e8);
            if !logic.is_null() && !matches!(read::<i32>(logic, 0x190), 5 | 7) {
                choose_state(logic, 5);
                withdrawn += 1;
            }
        }
        entry = next;
    }
    log::info!("aircraft_retreat: requested native withdrawal for {withdrawn} player aircraft");
}
