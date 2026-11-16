//! Restore the native 1.163 logos and let the player skip them with input.
//! The title tail and state-machine evidence is documented in hf_intro/README.md
//! inside the hf_intro 0.2.0 archive.

use std::{
    mem::{size_of, MaybeUninit},
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

use patchy::{PatchError, PatchSession, Result, ReturnType, Trampoline};
use windows_sys::Win32::{
    System::{
        LibraryLoader::GetModuleHandleA,
        Memory::{
            VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ,
            PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD,
        },
    },
    UI::Input::KeyboardAndMouse::GetAsyncKeyState,
};

const IMAGE_BASE: usize = 0x140000000;
const SKIP_BLOCK: usize = 0x1402ade6e;
const SKIP_BRANCH: usize = SKIP_BLOCK + 15;
const EPILOGUE: usize = 0x1402c7562;
const SCENE_STATE: usize = 0x147fc6fc4;
const MENU_STATE: i32 = 8;
const SKIP_GRACE: Duration = Duration::from_millis(1000);

const EXPECTED_SKIP_BLOCK: [u8; 46] = [
    0x48, 0x8d, 0x0d, 0x13, 0xb5, 0x73, 0x03, 0xe8, 0x36, 0x52, 0xda, 0xff, 0x48, 0x85, 0xc0, 0x0f,
    0x84, 0xdf, 0x96, 0x01, 0x00, 0x83, 0x3d, 0x3a, 0x91, 0xd1, 0x07, 0x08, 0x0f, 0x4f, 0x1d, 0x33,
    0x91, 0xd1, 0x07, 0x89, 0x1d, 0x2d, 0x91, 0xd1, 0x07, 0xe9, 0xc6, 0x96, 0x01, 0x00,
];
const EXPECTED_EPILOGUE: [u8; 15] = [
    0x48, 0x8b, 0x8d, 0xd0, 0x58, 0x00, 0x00, 0x48, 0x33, 0xcc, 0xe8, 0x2f, 0xd0, 0x06, 0x00,
];

static INTRO_STARTED: OnceLock<Instant> = OnceLock::new();
static PREVIOUS_KEYS: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

pub(crate) fn matches_executable_bytes(address: usize, expected: &[u8]) -> bool {
    let mut information = MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
    // SAFETY: VirtualQuery writes a properly aligned, sized output buffer.
    let queried = unsafe {
        VirtualQuery(
            address as *const _,
            information.as_mut_ptr(),
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if queried != size_of::<MEMORY_BASIC_INFORMATION>() {
        return false;
    }
    // SAFETY: VirtualQuery reported a complete initialized structure.
    let information = unsafe { information.assume_init() };
    if information.State != MEM_COMMIT || information.Protect & PAGE_GUARD != 0 {
        return false;
    }
    if !matches!(
        information.Protect,
        PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
    ) || information.AllocationBase as usize != IMAGE_BASE
    {
        return false;
    }
    let offset = address.saturating_sub(information.BaseAddress as usize);
    if expected.len() > information.RegionSize.saturating_sub(offset) {
        return false;
    }
    // SAFETY: The range is committed readable code in the main image. Modloader
    // prepares patches before the title path executes.
    unsafe { std::slice::from_raw_parts(address as *const u8, expected.len()) == expected }
}

unsafe extern "C" fn note_title_frame() {
    // SAFETY: The verified 1.163 title block addresses this global; the hook runs
    // on the same title thread as the original state clamp.
    let state = unsafe { ptr::read_volatile(SCENE_STATE as *const i32) };
    if state >= MENU_STATE {
        return;
    }
    let started = *INTRO_STARTED.get_or_init(Instant::now);
    let mut down = [0u64; 4];
    for code in 1..256usize {
        // SAFETY: GetAsyncKeyState reads the virtual-key state.
        if unsafe { GetAsyncKeyState(code as i32) } as u16 & 0x8000 != 0 {
            down[code / 64] |= 1 << (code % 64);
        }
    }
    let mut pressed = false;
    for (index, word) in down.iter().enumerate() {
        let previous = PREVIOUS_KEYS[index].swap(*word, Ordering::Relaxed);
        pressed |= word & !previous != 0;
    }
    if started.elapsed() < SKIP_GRACE || !pressed {
        return;
    }
    // SAFETY: This is the same title-thread global and value used by the
    // verified native clamp.
    unsafe { ptr::write_volatile(SCENE_STATE as *mut i32, MENU_STATE) };
    log::info!("Intro skip pressed");
}

pub unsafe fn prepare(session: &mut PatchSession) -> Result {
    // SAFETY: A null module name requests the main executable.
    if unsafe { GetModuleHandleA(ptr::null()) } as usize != IMAGE_BASE {
        return Err(PatchError::SourceChanged {
            address: IMAGE_BASE,
        });
    }
    if !matches_executable_bytes(SKIP_BLOCK, &EXPECTED_SKIP_BLOCK)
        || !matches_executable_bytes(EPILOGUE, &EXPECTED_EPILOGUE)
    {
        return Err(PatchError::SourceChanged {
            address: SKIP_BRANCH,
        });
    }

    let mut trampoline = Trampoline::new();
    trampoline.preserved_call(note_title_frame as *const (), &[], &[], ReturnType::None);
    trampoline.relative_jump(EPILOGUE);
    // SAFETY: The complete six-byte branch and its epilogue were verified.
    // The trampoline preserves volatile registers and resumes at that epilogue.
    unsafe { session.detour_trampoline(SKIP_BRANCH, 6, trampoline)? };
    log::info!("Prepared keyboard-skippable native intro");
    Ok(())
}
