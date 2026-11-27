//! Manual elevation (`m_floor`) control for the ship editor (Steam 1.163 only).
//!
//! While a part is held in the editor, the configured keys raise or lower its floor or give it
//! back to the game's automatic elevation solver. A readout shows the held part, its floor and its
//! mounted modules.
//!
//! The trampoline runs [`native_tick`] on the game thread after the editor label pass and before
//! the game flushes the draw target, so key polling, floor edits, and drawing happen on that thread.
//!
//! Addresses and offsets (Steam 1.163, image base `0x140000000`):
//!
//! | What                                  | Where                                   |
//! |---------------------------------------|-----------------------------------------|
//! | held part `Body*` (null if none)      | `0x1403c0d88`                           |
//! | editor-active flag                    | `0x1403ef368`                           |
//!
//! `m_floor_type` bit `0x01` makes the solver recompute the floor and `0x10` makes it force
//! floor 3. Parts with neither keep the floor stored in the ship.

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{c_char, CStr, CString},
    mem::offset_of,
    ptr,
    sync::OnceLock,
};

use highfleet::v1_163::Body;
use patchy::{CallBridge, FlagPolicy, KeepReturn, PatchSession, StackAlignment, Trampoline};

use windows_sys::Win32::{
    System::Threading::GetCurrentProcessId,
    UI::{
        Input::KeyboardAndMouse::GetAsyncKeyState,
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

use crate::config::{ElevationConfig, ResolvedKeys};

const HELD_PART: *const *mut Body = 0x1_403c_0d88usize as *const *mut Body;
const EDITOR_ACTIVE: *const u8 = 0x1_403e_f368usize as *const u8;

// Partial Steam 1.163 layout. highfleet-rs exposes the slot entries as ISerializable pointers.
#[repr(C)]
struct Slot {
    _unknown_00: [u8; 0x58],
    body: *mut Body,
    _unknown_60: [u8; 0x58],
    flags: u8,
}

const _: () = {
    assert!(offset_of!(Slot, body) == 0x58);
    assert!(offset_of!(Slot, flags) == 0xb8);
};

// Slot links with bit 2 set are sockets, not modules mounted on this part.
const SOCKET_FLAG: u8 = 1 << 2;

/// `m_floor_type` bits that make the solver recompute the floor.
const AUTO_BITS: u64 = 0x11;
const MIN_FLOOR: i32 = 0;

/// The `call 0x1400d4380` (editor label pass) that is wrapped, as `E8 rel32`.
const HOOK_SITE: usize = 0x1_4030_3fb4;
const HOOK_TARGET: usize = 0x1_400d_4380;
const HOOK_SIZE: usize = 5;

/// The global `hgeResourceManager*` (`Media.res`).
const MEDIA_RES: *const *mut HgeResourceManager =
    0x1_43a1_19e8usize as *const *mut HgeResourceManager;
/// `hgeResourceManager::GetFont(manager, name)`.
const GET_FONT: usize = 0x1_4017_03e0;
/// `(name, scale, blend, colour)`: gets a font and sets its scale, blend and colour (if non-zero).
const STYLE_FONT: usize = 0x1_401a_8710;
/// The game's modified `hgeFont::Render(font, x, y, align, text, *out_w, *out_h, ?, angle)`.
const RENDER: usize = 0x1_4016_eb40;

// Only passed back to the game; no resource-manager fields are accessed.
#[repr(C)]
struct HgeResourceManager {
    _opaque: [u8; 0],
}

// Partial layout of the game's modified hgeFont, not modeled in highfleet-rs.
#[repr(C)]
struct HgeFont {
    _unknown_0000: [u8; 0x100c],
    scale: f32,
    _unknown_1010: [u8; 0x10],
    color: u32,
    _unknown_1024: [u8; 4],
    blend: i32,
}

const _: () = {
    assert!(offset_of!(HgeFont, scale) == 0x100c);
    assert!(offset_of!(HgeFont, color) == 0x1020);
    assert!(offset_of!(HgeFont, blend) == 0x1028);
};

/// A small monospace pixel-style font from the game's own list (see hf_notes, fonts.md).
const HINT_FONT: &CStr = c"fixed_16";
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

type GetFontFn = unsafe extern "win64" fn(*mut HgeResourceManager, *const c_char) -> *mut HgeFont;
type StyleFn = unsafe extern "win64" fn(*const c_char, f32, i32, u32) -> *mut HgeFont;
type RenderFn = unsafe extern "win64" fn(
    *mut HgeFont,
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
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

thread_local! {
    static KEY_STATE: RefCell<KeyState> = RefCell::new(KeyState::default());
}

/// Reads the settings and prepares the editor hook.
pub unsafe fn prepare(session: &mut PatchSession, config: &ElevationConfig) -> patchy::Result {
    let keys = config.keys.resolve();
    let header = format!(
        "ELEVATION  {} up  {} down  {} auto\n",
        keys.raise_label, keys.lower_label, keys.auto_label
    );

    session.detour_trampoline(HOOK_SITE, HOOK_SIZE, |_| {
        let mut trampoline = Trampoline::new();
        // The original call site provides aligned RSP and shadow space.
        trampoline.absolute_call(HOOK_TARGET as *const ());
        trampoline.call(CallBridge {
            callback: native_tick as *const (),
            incoming_stack: StackAlignment::Aligned16,
            flags: FlagPolicy::Preserve,
            keep_return: KeepReturn::Neither,
            before_call: &[],
            after_call: &[],
        });
        trampoline.relative_jump(HOOK_SITE + HOOK_SIZE);
        Ok(trampoline)
    })?;

    let settings = Settings {
        max_floor: config.max_floor.max(MIN_FLOOR),
        sync_children: config.sync_children,
        child_offset: config.child_offset,
        keys,
        header,
    };
    log::info!(
        "Elevation prepared: {} raise, {} lower, {} auto (max floor {}, sync children {}, child offset {})",
        settings.keys.raise_label,
        settings.keys.lower_label,
        settings.keys.auto_label,
        settings.max_floor,
        settings.sync_children,
        settings.child_offset,
    );
    let _ = SETTINGS.set(settings);
    Ok(())
}

// --- the native hook ------------------------------------------------------------------------

/// Runs once per editor frame, right after the game drew its own editor labels and before it
/// flushes them. Handles the keys, then draws the readout into the same target.
unsafe extern "C" fn native_tick() {
    let Some(settings) = SETTINGS.get() else {
        return;
    };

    KEY_STATE.with(|state| {
        if let Ok(mut state) = state.try_borrow_mut() {
            poll_keys(settings, &mut state);
        }
    });
    draw_hint(settings);
}

/// Draws the readout with the game's own text code. Fonts are shared objects that every caller
/// restyles before use, so the style is saved and put back.
unsafe fn draw_hint(settings: &Settings) {
    let manager = MEDIA_RES.read();
    if manager.is_null() {
        return;
    }
    let get_font: GetFontFn = std::mem::transmute(GET_FONT);
    let font = get_font(manager, HINT_FONT.as_ptr());
    if font.is_null() {
        return;
    }

    let mut text = build_status(settings);
    let lines = text
        .lines()
        .take(MAX_HINT_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    // `@` is a line break for the game's text code and a NUL would end the text early.
    text = lines.replace(['@', '\0'], " ");
    let Ok(text) = CString::new(text) else { return };

    let old_scale = (*font).scale;
    let old_color = (*font).color;
    let old_blend = (*font).blend;

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

/// The held body is borrowed from the editor on the game thread, where the game keeps it live.
unsafe fn held_part() -> Option<*mut Body> {
    if EDITOR_ACTIVE.read_volatile() == 0 {
        return None;
    }
    let part = HELD_PART.read_volatile();
    (!part.is_null()).then_some(part)
}

unsafe fn oid(part: *const Body) -> String {
    let bytes = (*part).oid.get_string().as_bytes();
    String::from_utf8_lossy(&bytes[..bytes.len().min(40)]).into_owned()
}

/// Calls `visit(child, is_socket)` for every part linked to `parent` through its slot list.
///
/// Slots without the socket flag (bit 2) are mounted modules, the ones the game's floor solver
/// moves together with their mount.
unsafe fn for_each_linked(parent: *mut Body, mut visit: impl FnMut(*mut Body, bool)) {
    // The slot vector remains live and unchanged while the visitor reads or updates floors.
    for &entry in (*parent).slots.as_slice() {
        let slot = entry.cast::<Slot>();
        if slot.is_null() {
            continue;
        }

        let child = (*slot).body;
        if child.is_null() || ptr::eq(child, parent) {
            continue;
        }

        visit(child, (*slot).flags & SOCKET_FLAG != 0);
    }
}

// --- changing the elevation -----------------------------------------------------------------

fn clamp(value: i32, settings: &Settings) -> i32 {
    value.clamp(MIN_FLOOR, settings.max_floor)
}

unsafe fn adjust(delta: i32, settings: &Settings, original: &mut HashMap<usize, u64>) {
    let Some(part) = held_part() else { return };

    original.entry(part.addr()).or_insert((*part).floor_type);
    let before = (*part).floor;
    let next = clamp(before + delta, settings);
    (*part).floor_type &= !AUTO_BITS;
    (*part).floor = next;
    log::debug!(
        "Elevation: part {part:p} {}: floor {before} -> {next}, floor_type {:#x} -> {:#x}",
        oid(part),
        original[&part.addr()],
        (*part).floor_type
    );

    if settings.sync_children {
        for_each_linked(part, |child, socket| {
            if socket {
                return;
            }
            let target = clamp(next + settings.child_offset, settings);
            log::debug!(
                "Elevation:   mounted {child:p} {}: floor {} -> {target}",
                oid(child),
                (*child).floor
            );
            (*child).floor = target;
        });
    }
}

/// Puts the held part back under the game's solver: auto bit on, floors reset to 0 so the game
/// recomputes them the next time the ship is edited (drop the part to trigger that).
unsafe fn restore(original: &mut HashMap<usize, u64>) {
    let Some(part) = held_part() else { return };

    original.remove(&part.addr());
    // The "force floor 3" bit would pin the part again.
    (*part).floor_type = ((*part).floor_type | 0x1) & !0x10;
    let before = (*part).floor;
    (*part).floor = 0;
    log::debug!(
        "Elevation: part {part:p} {}: back to auto (floor {before} -> 0), floor_type {:#x}",
        oid(part),
        (*part).floor_type
    );

    for_each_linked(part, |child, socket| {
        if !socket {
            (*child).floor = 0;
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

// --- on-screen readout ----------------------------------------------------------------------

unsafe fn build_status(settings: &Settings) -> String {
    let mut status = settings.header.clone();

    let Some(part) = held_part() else {
        status.push_str("holding: -");
        return status;
    };

    let floor_type = (*part).floor_type;
    let floor = (*part).floor;
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
            (*child).floor
        ));
    });
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use highfleet::general::CVec;
    use std::mem::MaybeUninit;

    #[test]
    fn visits_only_links_to_other_bodies_with_their_socket_status() {
        let mut parent = MaybeUninit::<Body>::uninit();
        let mut mounted = MaybeUninit::<Body>::uninit();
        let mut neighbour = MaybeUninit::<Body>::uninit();
        let parent = parent.as_mut_ptr();
        let mounted = mounted.as_mut_ptr();
        let neighbour = neighbour.as_mut_ptr();

        let slot = |body, flags| Slot {
            _unknown_00: [0; 0x58],
            body,
            _unknown_60: [0; 0x58],
            flags,
        };
        let mut slots = [
            slot(mounted, 0),
            slot(parent, SOCKET_FLAG),
            slot(neighbour, SOCKET_FLAG),
            slot(ptr::null_mut(), 0),
        ];
        let mut entries = CVec::empty();
        entries.insert(ptr::null_mut());
        for slot in &mut slots {
            entries.insert(ptr::from_mut(slot).cast());
        }

        let mut links = Vec::new();
        unsafe {
            // Traversal only borrows the parent's initialized slots field and never dereferences
            // the children. Reclaim the vector without treating the rest of the Body as initialized.
            let field = &raw mut (*parent).slots;
            field.write(entries);
            for_each_linked(parent, |body, socket| links.push((body, socket)));
            field.drop_in_place();
        }

        assert_eq!(links, [(mounted, false), (neighbour, true)]);
    }
}
