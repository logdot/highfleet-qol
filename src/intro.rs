//! Restore the native 1.163 logos and let the player skip them with input.
//! The title tail and state-machine evidence is documented in hf_intro/README.md
//! inside the hf_intro 0.2.0 archive.

use std::{
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

use patchy::{PatchError, PatchSession, ProcessModule, Result, ReturnType, Trampoline};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

const IMAGE_BASE: usize = 0x140000000;
const SKIP_BLOCK: usize = 0x1402ade6e;
const SKIP_BRANCH: usize = SKIP_BLOCK + 15;
const EPILOGUE: usize = 0x1402c7562;
const SCENE_STATE: usize = 0x147fc6fc4;
const MENU_STATE: i32 = 8;
const SKIP_GRACE: Duration = Duration::from_millis(1000);
const KEY_DOWN_MASK: u16 = 0x8000;
const VIRTUAL_KEY_COUNT: usize = 256;

static INTRO_STARTED: OnceLock<Instant> = OnceLock::new();
static PREVIOUS_KEYS: [AtomicBool; VIRTUAL_KEY_COUNT] =
    [const { AtomicBool::new(false) }; VIRTUAL_KEY_COUNT];

fn check_key_pressed() -> bool {
    let mut pressed = false;

    // Track every key even during the grace period, so a held key does not
    // count as a fresh press when the period ends. Mouse buttons are virtual keys too.
    for (code, previous) in PREVIOUS_KEYS.iter().enumerate().skip(1) {
        // SAFETY: GetAsyncKeyState accepts virtual-key codes without pointer arguments.
        // Its high bit indicates whether the key is currently down.
        let down = unsafe { GetAsyncKeyState(code as i32) } as u16 & KEY_DOWN_MASK != 0;
        let was_down = previous.swap(down, Ordering::Relaxed);
        pressed |= down && !was_down;
    }

    pressed
}

unsafe extern "C" fn skip_title_on_input() {
    // SAFETY: The verified 1.163 title block addresses this global; the hook runs
    // on the same title thread as the original state clamp.
    let state = unsafe { ptr::read_volatile(SCENE_STATE as *const i32) };
    if state >= MENU_STATE {
        return;
    }

    let started = *INTRO_STARTED.get_or_init(Instant::now);
    let pressed = check_key_pressed();

    if started.elapsed() < SKIP_GRACE || !pressed {
        return;
    }

    // SAFETY: This is the same title-thread global and value used by the
    // verified native clamp.
    unsafe { ptr::write_volatile(SCENE_STATE as *mut i32, MENU_STATE) };
    log::info!("Intro skip pressed");
}

pub unsafe fn prepare(session: &mut PatchSession) -> Result {
    // The hook and scene-state addresses below are absolute, not relocatable.
    if ProcessModule::main()?.base_address() != IMAGE_BASE {
        return Err(PatchError::SourceChanged {
            address: IMAGE_BASE,
        });
    }

    let mut trampoline = Trampoline::new();
    trampoline.preserved_call(skip_title_on_input as *const (), &[], &[], ReturnType::None);
    trampoline.relative_jump(EPILOGUE);
    // SAFETY: Modloader identifies Steam 1.163, whose six-byte branch and
    // epilogue are documented in the intro research. The trampoline preserves
    // volatile registers and resumes at that epilogue.
    unsafe { session.detour_trampoline(SKIP_BRANCH, 6, trampoline)? };

    log::info!("Prepared keyboard-skippable native intro");
    Ok(())
}
