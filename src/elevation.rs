//! Manual elevation (`m_floor`) control for the ship editor (Steam 1.163 only).
//!
//! While a part is held in the editor, the configured keys raise or lower its floor or give it
//! back to the game's automatic elevation solver. A readout shows the held part, its floor and its
//! mounted modules.
//!
//! Two ways to run, chosen by the `hint` setting:
//!
//! * **native** (default): the `call 0x1400d4380` at `0x140303fb4` (E8 rel32, the editor's label
//!   pass, which draws into the same offscreen target the game flushes right afterwards) is wrapped
//!   by a trampoline: original call, then [`native_tick`]. That runs on the game thread once per
//!   editor frame, so it polls the keys, writes to the held part and draws the readout with the
//!   game's own font code. No extra threads and no extra window.
//! * **window** (and the fallback when the hook site does not look right): the original design of
//!   the standalone `highfleet-elevation` mod. A worker thread polls the keys and an overlay
//!   thread draws the readout in a transparent window above the game.
//!
//! With `hint: off` the native hook still handles the keys but draws nothing.
//!
//! Addresses and offsets (Steam 1.163, image base `0x140000000`):
//!
//! | What                                  | Where                                   |
//! |---------------------------------------|-----------------------------------------|
//! | held part `Body*` (null if none)      | `0x1403c0d88`                           |
//! | editor-active flag                    | `0x1403ef368`                           |
//! | `m_floor_type` bitmask / `m_floor`    | `Body + 0x9b0` / `Body + 0x9b8`         |
//! | `m_oid` string / its length           | `Body + 0x190` / `Body + 0x1a0`         |
//! | slot vector begin / end               | `Body + 0x350` / `Body + 0x358`         |
//! | slot flags (bit 2 = socket) / body    | `Slot + 0xb8` / `Slot + 0x58`           |
//!
//! `m_floor_type` bit `0x01` makes the solver recompute the floor and `0x10` makes it force
//! floor 3. Parts with neither keep the floor stored in the ship.

use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

use patchy::{PatchSession, ReturnType, Trampoline};

use windows_sys::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CreateFontA, CreateSolidBrush, DeleteObject, DrawTextA, EndPaint, FillRect,
        GetDC, InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
        ANTIALIASED_QUALITY, DEFAULT_CHARSET, DT_CALCRECT, FF_MODERN, FIXED_PITCH, FW_BOLD, HDC,
        PAINTSTRUCT, TRANSPARENT,
    },
    System::{
        LibraryLoader::GetModuleHandleA,
        Memory::{
            VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READWRITE,
            PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE, PAGE_WRITECOPY,
        },
        Threading::GetCurrentProcessId,
    },
    UI::{
        Input::KeyboardAndMouse::GetAsyncKeyState,
        WindowsAndMessaging::{
            CreateWindowExA, DefWindowProcA, DispatchMessageA, GetClientRect, GetForegroundWindow,
            GetMessageA, GetWindowThreadProcessId, LoadCursorW, RegisterClassA,
            SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, TranslateMessage,
            HWND_TOPMOST, IDC_ARROW, LWA_COLORKEY, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
            WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSA, WS_EX_LAYERED, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
};

use crate::config::{ElevationConfig, ElevationHint, ResolvedKeys};

const IMAGE_BASE: usize = 0x1_4000_0000;
const HELD_PART: usize = 0x1_403c_0d88;
const EDITOR_ACTIVE: usize = 0x1_403e_f368;

const OFF_OID: usize = 0x190;
const OFF_OID_LEN: usize = 0x1a0;
const OFF_FLOOR_TYPE: usize = 0x9b0;
const OFF_FLOOR: usize = 0x9b8;
const OFF_SLOTS_BEGIN: usize = 0x350;
const OFF_SLOTS_END: usize = 0x358;
const SLOT_FLAGS: usize = 0xb8;
const SLOT_BODY: usize = 0x58;

/// `m_floor_type` bits that make the solver recompute the floor.
const AUTO_BITS: u64 = 0x11;
const MIN_FLOOR: i32 = 0;

/// Pixel colour that is drawn as transparent in the overlay window.
const COLOR_KEY: COLORREF = 0x00ff_00ff;

/// The `call 0x1400d4380` (editor label pass) that is wrapped, as `E8 rel32`.
const HOOK_SITE: usize = 0x1_4030_3fb4;
const HOOK_TARGET: usize = 0x1_400d_4380;
const HOOK_BYTES: [u8; 5] = [0xe8, 0xc7, 0x03, 0xdd, 0xff];

/// The global `hgeResourceManager*` (`Media.res`).
const MEDIA_RES: usize = 0x1_43a1_19e8;
/// `hgeResourceManager::GetFont(manager, name)`.
const GET_FONT: usize = 0x1_4017_03e0;
/// `(name, scale, blend, colour)`: gets a font and sets its scale, blend and colour (if non-zero).
const STYLE_FONT: usize = 0x1_401a_8710;
/// The game's modified `hgeFont::Render(font, x, y, align, text, *out_w, *out_h, ?, angle)`.
const RENDER: usize = 0x1_4016_eb40;

const OFF_FONT_SCALE: usize = 0x100c;
const OFF_FONT_COLOR: usize = 0x1020;
const OFF_FONT_BLEND: usize = 0x1028;

/// The font most of the editor's own labels use.
const HINT_FONT: &CStr = c"dinpro_14_bold";
/// Opaque white (`0xAARRGGBB`).
const HINT_COLOR: u32 = 0xffff_ffff;
/// Normal alpha blending (`BLEND_DEFAULT`).
const HINT_BLEND: i32 = 2;
/// The game draws in a 1920x1080 frame that is scaled to the screen. The right side of the editor
/// is free, so the readout hangs from the top right corner.
const HINT_X: f32 = 1880.0;
const HINT_Y: f32 = 24.0;
const ALIGN_RIGHT: u32 = 1;
/// Longest readout, so a part with very many mounted modules cannot run off the screen.
const MAX_HINT_LINES: usize = 12;

type GetFontFn = unsafe extern "win64" fn(*mut c_void, *const c_char) -> *mut c_void;
type StyleFn = unsafe extern "win64" fn(*const c_char, f32, i32, u32) -> *mut c_void;
type RenderFn = unsafe extern "win64" fn(
    *mut c_void,
    f32,
    f32,
    u32,
    *const c_char,
    *mut f32,
    *mut f32,
    u64,
    f32,
) -> i32;

struct Settings {
    max_floor: i32,
    sync_children: bool,
    child_offset: i32,
    keys: ResolvedKeys,
    /// First lines of the readout: the title and the keys.
    header: String,
    /// How the readout is shown after checking that the native hook site is usable.
    hint: ElevationHint,
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();
/// Key and part state of the native hook, which only ever runs on the game thread.
static NATIVE_STATE: OnceLock<Mutex<KeyState>> = OnceLock::new();
static STATUS: Mutex<String> = Mutex::new(String::new());

/// Prepares manual elevation: checks the game image, reads the settings and, unless the hint is
/// set to `window`, prepares the native hook. Nothing is started yet; see [`start`].
pub unsafe fn prepare(session: &mut PatchSession, config: &ElevationConfig) -> patchy::Result {
    if GetModuleHandleA(std::ptr::null()) as usize != IMAGE_BASE {
        log::error!("Elevation: unexpected image base, not enabling");
        return Ok(());
    }

    let keys = config.keys.resolve();
    let header = format!(
        "ELEVATION  {} up  {} down  {} auto\n",
        keys.raise_label, keys.lower_label, keys.auto_label
    );

    let mut hint = config.hint;
    if hint != ElevationHint::Window {
        // If another mod (or the debug draw test) already changed this call, a failed patch would
        // stop every QoL feature from installing, so look first and fall back to the window.
        let site = std::slice::from_raw_parts(HOOK_SITE as *const u8, HOOK_BYTES.len());
        if site != HOOK_BYTES {
            log::error!(
                "Elevation: the editor label call at {HOOK_SITE:#x} is not what was expected, using the overlay window instead"
            );
            hint = ElevationHint::Window;
        }
    }

    if hint != ElevationHint::Window {
        let mut trampoline = Trampoline::new();
        trampoline.absolute_call(HOOK_TARGET as *const ());
        trampoline.preserved_call(native_tick as *const (), &[], &[], ReturnType::None);
        trampoline.relative_jump(HOOK_SITE + HOOK_BYTES.len());
        session.detour_trampoline(HOOK_SITE, HOOK_BYTES.len(), trampoline)?;
        let _ = NATIVE_STATE.set(Mutex::new(KeyState::default()));
    }

    let settings = Settings {
        max_floor: config.max_floor.max(MIN_FLOOR),
        sync_children: config.sync_children,
        child_offset: config.child_offset,
        keys,
        header,
        hint,
    };
    log::info!(
        "Elevation prepared: {} raise, {} lower, {} auto (max floor {}, sync children {}, child offset {}, hint {:?})",
        settings.keys.raise_label,
        settings.keys.lower_label,
        settings.keys.auto_label,
        settings.max_floor,
        settings.sync_children,
        settings.child_offset,
        settings.hint
    );
    let _ = SETTINGS.set(settings);
    Ok(())
}

/// Starts the key thread and the overlay window when the hint is `window`. The native hook needs
/// no threads, so this does nothing for the other modes.
pub unsafe fn start() {
    let Some(settings) = SETTINGS.get() else {
        return;
    };
    if settings.hint != ElevationHint::Window {
        return;
    }

    if let Err(error) = thread::Builder::new()
        .name("qol-elevation".into())
        .spawn(|| unsafe { worker() })
    {
        log::error!("Elevation: unable to start the key thread: {error}");
        return;
    }
    if let Err(error) = thread::Builder::new()
        .name("qol-elevation-overlay".into())
        .spawn(|| unsafe { overlay_thread() })
    {
        log::error!("Elevation: unable to start the overlay thread: {error}");
    }
}

// --- the native hook ------------------------------------------------------------------------

/// Runs once per editor frame, right after the game drew its own editor labels and before it
/// flushes them. Handles the keys, then draws the readout into the same target.
unsafe extern "C" fn native_tick() {
    let Some(settings) = SETTINGS.get() else {
        return;
    };
    let Some(state) = NATIVE_STATE.get() else {
        return;
    };
    if let Ok(mut state) = state.lock() {
        poll_keys(settings, &mut state);
    }
    if settings.hint == ElevationHint::Native {
        draw_hint(settings);
    }
}

/// Draws the readout with the game's own text code. Fonts are shared objects that every caller
/// restyles before use, so the style is saved and put back.
unsafe fn draw_hint(_settings: &Settings) {
    let manager: *mut c_void = read(MEDIA_RES);
    if manager.is_null() {
        return;
    }
    let get_font: GetFontFn = std::mem::transmute(GET_FONT);
    let font = get_font(manager, HINT_FONT.as_ptr());
    if font.is_null() {
        return;
    }

    let mut text = build_status();
    let lines = text
        .lines()
        .take(MAX_HINT_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    // `@` is a line break for the game's text code and a NUL would end the text early.
    text = lines.replace(['@', '\0'], " ");
    let Ok(text) = CString::new(text) else { return };

    let base = font as usize;
    let old_scale: f32 = read(base + OFF_FONT_SCALE);
    let old_color: u32 = read(base + OFF_FONT_COLOR);
    let old_blend: i32 = read(base + OFF_FONT_BLEND);

    let style: StyleFn = std::mem::transmute(STYLE_FONT);
    style(HINT_FONT.as_ptr(), 1.0, HINT_BLEND, HINT_COLOR);
    let render: RenderFn = std::mem::transmute(RENDER);
    render(
        font,
        HINT_X,
        HINT_Y,
        ALIGN_RIGHT,
        text.as_ptr(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        0,
        0.0,
    );
    style(HINT_FONT.as_ptr(), old_scale, old_blend, old_color);
}

// --- reading the game's memory --------------------------------------------------------------

unsafe fn readable(address: usize, length: usize) -> bool {
    let mut info: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
    if VirtualQuery(
        address as *const _,
        &mut info,
        std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
    ) == 0
    {
        return false;
    }
    if info.State != MEM_COMMIT || info.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
        return false;
    }
    // Image data is usually write-copy, heap memory is read-write.
    let writable =
        PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    if info.Protect & writable == 0 {
        return false;
    }
    address + length <= info.BaseAddress as usize + info.RegionSize
}

unsafe fn read<T: Copy>(address: usize) -> T {
    (address as *const T).read_unaligned()
}

unsafe fn editor_active() -> bool {
    std::ptr::read_volatile(EDITOR_ACTIVE as *const u8) != 0
}

/// A part always has a non-empty `m_oid` of at most 64 characters.
unsafe fn looks_like_body(part: usize) -> bool {
    if part == 0 || !readable(part, OFF_FLOOR + 4) {
        return false;
    }
    let length: u64 = read(part + OFF_OID_LEN);
    length != 0 && length <= 64
}

/// The part held in the editor, if it looks like a valid `Body`.
unsafe fn held_part() -> Option<usize> {
    if !editor_active() {
        return None;
    }
    let part = std::ptr::read_volatile(HELD_PART as *const usize);
    looks_like_body(part).then_some(part)
}

unsafe fn oid(part: usize) -> String {
    let length: u64 = read(part + OFF_OID_LEN);
    let capacity: u64 = read(part + OFF_OID_LEN + 8);
    let length = length.min(40) as usize;
    let text = if capacity >= 16 {
        let pointer: usize = read(part + OFF_OID);
        if !readable(pointer, length) {
            return "?".into();
        }
        pointer
    } else {
        part + OFF_OID
    };
    String::from_utf8_lossy(std::slice::from_raw_parts(text as *const u8, length)).into_owned()
}

/// Calls `visit(child, is_socket)` for every part linked to `parent` through its slot list.
///
/// Slots without the socket flag (bit 2) are mounted modules, the ones the game's floor solver
/// moves together with their mount.
unsafe fn for_each_linked(parent: usize, mut visit: impl FnMut(usize, bool)) {
    if !readable(parent + OFF_SLOTS_BEGIN, 16) {
        return;
    }
    let begin: usize = read(parent + OFF_SLOTS_BEGIN);
    let end: usize = read(parent + OFF_SLOTS_END);
    if begin == 0 || end < begin || (end - begin) / 8 > 64 {
        return;
    }

    let mut entry = begin;
    while entry < end {
        if !readable(entry, 8) {
            return;
        }
        let slot: usize = read(entry);
        entry += 8;
        if slot == 0 || !readable(slot, SLOT_BODY + 8) {
            continue;
        }
        let socket = read::<u8>(slot + SLOT_FLAGS) & 4 != 0;
        let child: usize = read(slot + SLOT_BODY);
        if child == 0 || child == parent || !looks_like_body(child) {
            continue;
        }
        visit(child, socket);
    }
}

// --- changing the elevation -----------------------------------------------------------------

fn clamp(value: i32, settings: &Settings) -> i32 {
    value.clamp(MIN_FLOOR, settings.max_floor)
}

unsafe fn adjust(delta: i32, settings: &Settings, original: &mut HashMap<usize, u64>) {
    let Some(part) = held_part() else { return };
    let floor_type = (part + OFF_FLOOR_TYPE) as *mut u64;
    let floor = (part + OFF_FLOOR) as *mut i32;

    original.entry(part).or_insert(*floor_type);
    let before = *floor;
    let next = clamp(before + delta, settings);
    *floor_type &= !AUTO_BITS;
    *floor = next;
    log::debug!(
        "Elevation: part {part:#x} {}: floor {before} -> {next}, floor_type {:#x} -> {:#x}",
        oid(part),
        original[&part],
        *floor_type
    );

    if settings.sync_children {
        for_each_linked(part, |child, socket| {
            if socket {
                return;
            }
            let child_floor = (child + OFF_FLOOR) as *mut i32;
            let target = clamp(next + settings.child_offset, settings);
            log::debug!(
                "Elevation:   mounted {child:#x} {}: floor {} -> {target}",
                oid(child),
                *child_floor
            );
            *child_floor = target;
        });
    }
}

/// Puts the held part back under the game's solver: auto bit on, floors reset to 0 so the game
/// recomputes them the next time the ship is edited (drop the part to trigger that).
unsafe fn restore(original: &mut HashMap<usize, u64>) {
    let Some(part) = held_part() else { return };
    let floor_type = (part + OFF_FLOOR_TYPE) as *mut u64;
    let floor = (part + OFF_FLOOR) as *mut i32;

    original.remove(&part);
    // The "force floor 3" bit would pin the part again.
    *floor_type = (*floor_type | 0x1) & !0x10;
    let before = *floor;
    *floor = 0;
    log::debug!(
        "Elevation: part {part:#x} {}: back to auto (floor {before} -> 0), floor_type {:#x}",
        oid(part),
        *floor_type
    );

    for_each_linked(part, |child, socket| {
        if !socket {
            *((child + OFF_FLOOR) as *mut i32) = 0;
        }
    });
}

unsafe fn key_down(code: u16) -> bool {
    GetAsyncKeyState(code as i32) as u16 & 0x8000 != 0
}

/// Remembers which keys were down on the previous poll so that only a new press counts, and the
/// `m_floor_type` of the parts that were changed.
#[derive(Default)]
struct KeyState {
    raise_was: bool,
    lower_was: bool,
    auto_was: bool,
    original: HashMap<usize, u64>,
}

/// True when the game's window has the keyboard focus. `GetAsyncKeyState` sees keys typed into
/// any program, so keys are ignored while another window is active.
unsafe fn game_has_focus() -> bool {
    let mut process = 0u32;
    GetWindowThreadProcessId(GetForegroundWindow(), &mut process);
    process == GetCurrentProcessId()
}

/// Applies the keys that were newly pressed since the last call.
unsafe fn poll_keys(settings: &Settings, state: &mut KeyState) {
    let focused = game_has_focus();
    let lower = focused && key_down(settings.keys.lower);
    let raise = focused && key_down(settings.keys.raise);
    let auto = focused && key_down(settings.keys.auto);

    if lower && !state.lower_was {
        adjust(-1, settings, &mut state.original);
    }
    if raise && !state.raise_was {
        adjust(1, settings, &mut state.original);
    }
    if auto && !state.auto_was {
        restore(&mut state.original);
    }
    state.raise_was = raise;
    state.lower_was = lower;
    state.auto_was = auto;
}

unsafe fn worker() {
    let Some(settings) = SETTINGS.get() else {
        return;
    };
    let mut state = KeyState::default();

    loop {
        thread::sleep(Duration::from_millis(15));
        poll_keys(settings, &mut state);
    }
}

// --- on-screen readout ----------------------------------------------------------------------

unsafe fn build_status() -> String {
    let mut status = SETTINGS
        .get()
        .map(|settings| settings.header.clone())
        .unwrap_or_default();

    let Some(part) = held_part() else {
        status.push_str("holding: -");
        return status;
    };

    let floor_type: u64 = read(part + OFF_FLOOR_TYPE);
    let floor: i32 = read(part + OFF_FLOOR);
    status.push_str(&format!(
        "holding: {}  floor={floor}  type={floor_type:#x} ({})",
        oid(part),
        if floor_type & AUTO_BITS != 0 {
            "auto"
        } else {
            "manual"
        }
    ));
    for_each_linked(part, |child, socket| {
        status.push_str(&format!(
            "\n  {} {}  floor={}",
            if socket { "neighbour" } else { "mounted  " },
            oid(child),
            read::<i32>(child + OFF_FLOOR)
        ));
    });
    status
}

unsafe fn make_font() -> *mut core::ffi::c_void {
    CreateFontA(
        18,
        0,
        0,
        0,
        FW_BOLD as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        0,
        0,
        ANTIALIASED_QUALITY as u32,
        (FIXED_PITCH | FF_MODERN) as u32,
        c"Consolas".as_ptr().cast(),
    )
}

unsafe fn draw_text(dc: HDC, text: &mut [u8], rect: &mut RECT, flags: u32) {
    DrawTextA(dc, text.as_mut_ptr(), text.len() as i32, rect, flags);
}

unsafe extern "system" fn overlay_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            if !editor_active() {
                ShowWindow(window, SW_HIDE);
                return 0;
            }
            let status = build_status();
            let mut text = status.clone().into_bytes();
            if let Ok(mut shared) = STATUS.lock() {
                *shared = status;
            }

            let dc = GetDC(window);
            let font = make_font();
            let old = SelectObject(dc, font);
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            };
            draw_text(dc, &mut text, &mut rect, DT_CALCRECT);
            SelectObject(dc, old);
            DeleteObject(font);
            ReleaseDC(window, dc);

            SetWindowPos(
                window,
                HWND_TOPMOST,
                12,
                12,
                rect.right + 8,
                rect.bottom + 6,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(window, std::ptr::null(), 1);
            0
        }
        WM_ERASEBKGND => {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(window, &mut rect);
            let brush = CreateSolidBrush(COLOR_KEY);
            FillRect(wparam as HDC, &rect, brush);
            DeleteObject(brush);
            1
        }
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(window, &mut paint);
            let font = make_font();
            let old = SelectObject(dc, font);
            SetBkMode(dc, TRANSPARENT as i32);

            let status = STATUS.lock().map(|s| s.clone()).unwrap_or_default();
            let mut text = status.into_bytes();
            let mut rect = RECT {
                left: 2,
                top: 2,
                right: 2000,
                bottom: 2000,
            };
            SetTextColor(dc, 0x000a_0a0a);
            draw_text(dc, &mut text, &mut rect, 0);
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 2000,
                bottom: 2000,
            };
            SetTextColor(dc, 0x0078_ff78);
            draw_text(dc, &mut text, &mut rect, 0);

            SelectObject(dc, old);
            DeleteObject(font);
            EndPaint(window, &paint);
            0
        }
        _ => DefWindowProcA(window, message, wparam, lparam),
    }
}

/// Creates the click-through overlay and pumps its messages. The window must be created and
/// serviced on the same thread, so this never returns.
unsafe fn overlay_thread() {
    let instance = GetModuleHandleA(std::ptr::null());
    let class_name = c"HFElevationOverlay";
    let class = WNDCLASSA {
        style: 0,
        lpfnWndProc: Some(overlay_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr().cast(),
    };
    RegisterClassA(&class);

    let window = CreateWindowExA(
        WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class_name.as_ptr().cast(),
        c"".as_ptr().cast(),
        WS_POPUP,
        12,
        12,
        400,
        60,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        instance,
        std::ptr::null(),
    );
    if window.is_null() {
        log::error!("Elevation: unable to create the overlay window");
        return;
    }

    SetLayeredWindowAttributes(window, COLOR_KEY, 0, LWA_COLORKEY);
    SetTimer(window, 1, 100, None);
    log::info!("Elevation: overlay created");

    let mut message: MSG = std::mem::zeroed();
    while GetMessageA(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&message);
        DispatchMessageA(&message);
    }
}
