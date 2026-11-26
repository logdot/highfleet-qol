//! Reworks the battle **CRAFTS / MUNITION** panel so every aircraft type is listed, not just the
//! T-7 and LA-29.
//!
//! The panel is drawn once per frame by `Hud_DrawMapSignLabels` (`0x14029b3e0`), called from
//! `combatEvents` at `0x14029a8f6`. Its aircraft list is not data: the oid of every part is
//! compared against two hardcoded strings (`"CRAFT_LA29"` and `"CRAFT_T7"`), each feeding its own
//! counter register and its own hand-written draw block. A third type - for example a QoL aircraft
//! such as `CRAFT_SB15` - can never be counted and never gets an icon, so the panel shows nothing
//! for it.
//!
//! With this option enabled, that call is replaced by the same drawing driven by a list:
//!
//! - an aircraft type is shown when its oid is a key of the aircraft table rebuilt from the
//!   `planes` section of `qol.json` (`0x143a13c50`), so a craft that cannot be armed stays hidden;
//! - the icon is the animation `sign_<aircraft name>` (`CRAFT_SB15` -> `sign_SB15`), the same
//!   convention as the game's own `sign_T7` / `sign_LA29`. A missing animation leaves the slot
//!   empty and still prints the count;
//! - art larger than 58 x 73 is shrunk about its hotspot to fit; anything that fits is drawn at its
//!   own size;
//! - three types per row, up to three rows. A second row moves the `CRAFTS` caption, the divider,
//!   the `MUNITION` caption and the munitions row down by one row pitch, exactly like the rest of
//!   the layout does for the first row;
//! - counts and munitions are the same numbers the game itself uses (`Stats.m_category` aircraft
//!   bit, and the `AircraftLogic` round counters), and a type with zero planes stays hidden.
//!
//! Everything else is the game's own geometry and the game's own draw calls: player at x = 30,
//! enemy at `graphicsFrameWidth - 400`, counts at y = 40, icons at y = 75, captions at 107 / 132,
//! divider at 130, munitions at 155 / 190, entries advanced by `icon_x + 50` with the icon clearing
//! the count by 35. The one addition to the enemy side is that its left edge is pulled left when
//! its widest row would otherwise run off the screen, because both rows grow rightwards.
//!
//! Only Steam 1.163 is supported; on 1.151 this module is not compiled.

use std::ffi::{c_char, CString};
use std::mem::{size_of, MaybeUninit};
use std::slice;
use std::sync::{Arc, Mutex, OnceLock};

use patchy::{PatchError, PatchSession, ProcessModule, Result, Trampoline};
use windows_sys::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS,
};

/// `CALL Hud_DrawMapSignLabels` inside `combatEvents`: `e8 e5 0a 00 00`.
const SITE_RVA: usize = 0x29a8f6;
const STOLEN: [u8; 5] = [0xe8, 0xe5, 0x0a, 0x00, 0x00];
const TARGET_RVA: usize = 0x29b3e0;
const CONTINUATION_RVA: usize = 0x29a8fb;

// Every function this module calls, relative to the module base.
const BODY_MARKERS_RVA: usize = 0x29ae40;
const LOAD_MEDIA_RVA: usize = 0x170230;
const SET_SPRITE_FIELDS_RVA: usize = 0x172ae0;
const QUAD_POSITION_RVA: usize = 0x1725c0;
const QUAD_RENDER_EX_RVA: usize = 0x172aa0;
const FONT_SETUP_RVA: usize = 0x1a8710;
const FONT_SET_COLOR_RVA: usize = 0x16f120;
const MEASURE_STRING_RVA: usize = 0x16e9f0;
const DRAW_LABEL_RVA: usize = 0x16eb40;
const DRAW_WORLD_LINE_RVA: usize = 0x248ba0;
const CHECK_PART_FLAG_RVA: usize = 0x49470;

// Globals, relative to the module base.
const MEDIA_MANAGER_RVA: usize = 0x3a119e8;
const GRAPHICS_WIDTH_RVA: usize = 0x3a158fc;
const PLANE_TABLE_RVA: usize = 0x3a13c50;

/// Player roster: `{ list head, length }`, built by the combat level scripts.
const PLAYER_ROSTER: Roster = Roster {
    list: 0x7fc73b0,
    length: 0x7fc73b8,
};
/// Enemy roster.
const ENEMY_ROSTER: Roster = Roster {
    list: 0x7fc75a8,
    length: 0x7fc75b0,
};

/// A side's battle roster globals (RVAs).
#[derive(Clone, Copy, Debug)]
struct Roster {
    list: usize,
    length: usize,
}

/// `TllContainer<EscadraString, Plane>` node layout: links, flags, key, payload.
const TLL_LEFT: usize = 0x00;
const TLL_PARENT: usize = 0x08;
const TLL_RIGHT: usize = 0x10;
const TLL_IS_SENTINEL: usize = 0x19;
const TLL_KEY: usize = 0x20;
const TLL_NODE_SIZE: usize = 0x60;

/// Roster list node -> `Body *`, and the body fields this module reads.
const NODE_BODY: usize = 0x18;
const BODY_CODE: usize = 0x28;
const BODY_OID: usize = 0x190;
const BODY_AIRCRAFT_LOGIC: usize = 0xa20;
const BODY_SIZE: usize = 0xa30;
const CODE_BODY: i32 = 15;
/// `Stats.m_category` bit that marks the aircraft category.
const CATEGORY_AIRCRAFT: u32 = 0x400;

/// `AircraftLogic` counters the panel sums (bombs, rockets, K-13, gun rounds).
const LOGIC_BOMBS: usize = 0x128;
const LOGIC_ROCKETS: usize = 0x138;
const LOGIC_AAM: usize = 0x148;
const LOGIC_GUN_LOAD: usize = 0xac;
const LOGIC_GUN_AMMO: usize = 0xb4;
const ROUNDS_PER_GUN_AMMO: i32 = 20;

/// `GfxQuad` fields: blend state cleared on every sign load, and the frame's texture extents.
const SPRITE_BLEND: usize = 0x168;
const SPRITE_EXTENT_X: usize = 0x188;
const SPRITE_EXTENT_Y: usize = 0x18c;

// The panel's own geometry, all of it the game's.
const PLAYER_X: f32 = 30.0;
const ENEMY_INSET: f32 = 400.0;
const EDGE_INSET: f32 = 30.0;
const FALLBACK_WIDTH: i32 = 1920;
const LABEL_Y: f32 = 40.0;
const ICON_Y: f32 = 75.0;
const CRAFTS_CAPTION_Y: f32 = 107.0;
const LINE_Y: f32 = 130.0;
const MUNITION_CAPTION_Y: f32 = 132.0;
const MUNITION_LABEL_Y: f32 = 155.0;
const MUNITION_ICON_Y: f32 = 190.0;
const LINE_LENGTH: f32 = 240.0;
const LINE_WIDTH: f32 = 5.0;
/// `icon_x + 50`, the game's own advance between entries.
const ENTRY_ADVANCE: f32 = 50.0;
const DIVIDER_PAD: f32 = 8.0;
/// Three types per row, three rows.
const COLUMNS: usize = 3;
const MAX_ROWS: usize = 3;
const COLUMN_PITCH: f32 = 85.0;
const ROW_PITCH: f32 = 85.0;
/// Where the icon sits relative to its count: `label_x + text width + 35`.
const ICON_OFFSET: f32 = 35.0;
/// Art bigger than this is shrunk to fit; the game's own `sign_T7` is 58 x 73.
const ICON_MAX_WIDTH: f32 = 58.0;
const ICON_MAX_HEIGHT: f32 = 73.0;

/// The colour the game creates both panel fonts with; each side recolours before it draws.
const FONT_COLOR: u32 = ENEMY_COLOR;
/// The panel's own side colours.
const PLAYER_COLOR: u32 = 0xFF00_E4FF;
const ENEMY_COLOR: u32 = 0xFFA8_FC00;

const FONT_COUNT: &[u8] = b"pollock_60\0";
const FONT_CAPTION: &[u8] = b"dinpro_14_reg\0";

type FnBodyMarkers = unsafe extern "C" fn();
type FnLoadMedia = unsafe extern "C" fn(usize, *const c_char) -> usize;
type FnSetSpriteFields = unsafe extern "C" fn(usize, u32, i32);
type FnQuadPosition = unsafe extern "C" fn(usize, f32, f32);
type FnQuadRenderEx = unsafe extern "C" fn(usize, f32, f32, f32, f32, f32);
type FnSetupFont = unsafe extern "C" fn(*const c_char, f32, i32, u32) -> usize;
type FnSetFontColor = unsafe extern "C" fn(usize, u32);
type FnMeasureString = unsafe extern "C" fn(usize, *const u8, i32) -> f32;
type FnDrawLabel =
    unsafe extern "C" fn(usize, f32, f32, u32, *const u8, *mut f32, usize, f32, f32) -> i32;
type FnDrawWorldLine = unsafe extern "C" fn(Vec2, Vec2, f32, u32, u32, u8, f32, u32, f32);
type FnPartHasCategory = unsafe extern "C" fn(usize, u32) -> bool;

/// A 2D point by value - `Gfx_DrawWorldLine` takes two of them in `RCX` / `RDX`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// The munition icons in the game's order, with the counter each one shows.
const MUNITIONS: [(&str, fn(&Totals) -> i32); 4] = [
    ("sign_GPB", |totals| totals.bombs),
    ("sign_NAR", |totals| totals.rockets),
    ("sign_AAM", |totals| totals.aam),
    ("sign_GUN", |totals| totals.gun),
];

/// Runtime addresses, resolved once when the patch is prepared.
struct Game {
    body_markers: usize,
    load_media: usize,
    set_sprite_fields: usize,
    quad_position: usize,
    quad_render_ex: usize,
    font_setup: usize,
    font_set_color: usize,
    measure_string: usize,
    draw_label: usize,
    draw_world_line: usize,
    check_part_flag: usize,
    media_manager: usize,
    graphics_width: usize,
    plane_table: usize,
    player_roster: Roster,
    enemy_roster: Roster,
}

impl Game {
    fn new(module: ProcessModule) -> Result<Self> {
        Ok(Self {
            body_markers: module.resolve_rva(BODY_MARKERS_RVA)?,
            load_media: module.resolve_rva(LOAD_MEDIA_RVA)?,
            set_sprite_fields: module.resolve_rva(SET_SPRITE_FIELDS_RVA)?,
            quad_position: module.resolve_rva(QUAD_POSITION_RVA)?,
            quad_render_ex: module.resolve_rva(QUAD_RENDER_EX_RVA)?,
            font_setup: module.resolve_rva(FONT_SETUP_RVA)?,
            font_set_color: module.resolve_rva(FONT_SET_COLOR_RVA)?,
            measure_string: module.resolve_rva(MEASURE_STRING_RVA)?,
            draw_label: module.resolve_rva(DRAW_LABEL_RVA)?,
            draw_world_line: module.resolve_rva(DRAW_WORLD_LINE_RVA)?,
            check_part_flag: module.resolve_rva(CHECK_PART_FLAG_RVA)?,
            media_manager: module.resolve_rva(MEDIA_MANAGER_RVA)?,
            graphics_width: module.resolve_rva(GRAPHICS_WIDTH_RVA)?,
            plane_table: module.resolve_rva(PLANE_TABLE_RVA)?,
            player_roster: Roster {
                list: module.resolve_rva(PLAYER_ROSTER.list)?,
                length: module.resolve_rva(PLAYER_ROSTER.length)?,
            },
            enemy_roster: Roster {
                list: module.resolve_rva(ENEMY_ROSTER.list)?,
                length: module.resolve_rva(ENEMY_ROSTER.length)?,
            },
        })
    }
}

static GAME: OnceLock<Game> = OnceLock::new();

/// Replaces the battle CRAFTS / MUNITION panel so every aircraft in the `planes` section gets an
/// icon.
///
/// # Safety
///
/// Installs a detour into the running game's `.text`; it must only be called from the mod loader's
/// `init`, exactly like the other patches in this crate.
pub unsafe fn patch_plane_icons(session: &mut PatchSession) -> Result {
    let module = ProcessModule::main()?;
    let site = module.resolve_rva(SITE_RVA)?;
    let found = slice::from_raw_parts(site as *const u8, STOLEN.len());

    if found != STOLEN {
        return Err(PatchError::SourceChanged { address: site });
    }
    let relative = i32::from_le_bytes(STOLEN[1..].try_into().unwrap()) as isize;
    if (SITE_RVA + STOLEN.len()) as isize + relative != TARGET_RVA as isize {
        // A different build: the call no longer reaches the panel.
        return Err(PatchError::SourceChanged { address: site });
    }

    let game = Game::new(module)?;
    let continuation = module.resolve_rva(CONTINUATION_RVA)?;
    let _ = GAME.set(game);

    session.detour_trampoline(site, STOLEN.len(), build_trampoline(continuation))?;

    log::info!("Prepared plane icons rework");
    Ok(())
}

fn build_trampoline(continuation: usize) -> Trampoline {
    let mut trampoline = Trampoline::new();
    // The replaced instruction is a mid-function CALL: RSP is 0 mod 16 there, so no stack padding.
    trampoline.absolute_call(draw_hook as *const ());
    trampoline.relative_jump(continuation);
    trampoline
}

extern "C" fn draw_hook() {
    let _ = std::panic::catch_unwind(|| unsafe {
        if let Some(game) = GAME.get() {
            draw(game);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Game memory
// ---------------------------------------------------------------------------------------------

/// True when `size` bytes at `address` are committed, readable and guard-free.
fn readable(address: usize, size: usize) -> bool {
    if address < 0x1_0000 || size == 0 {
        return false;
    }
    let mut information = MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
    // SAFETY: VirtualQuery only inspects an address; the buffer is sized and aligned for the
    // structure it fills in.
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
    // SAFETY: VirtualQuery returned a complete structure.
    let information = unsafe { information.assume_init() };
    if information.State != MEM_COMMIT
        || information.Protect == PAGE_NOACCESS
        || information.Protect & PAGE_GUARD != 0
    {
        return false;
    }
    let region_offset = address.saturating_sub(information.BaseAddress as usize);
    size <= information.RegionSize.saturating_sub(region_offset)
}

fn usize_at(address: usize) -> Option<usize> {
    readable(address, size_of::<usize>()).then(|| unsafe {
        // SAFETY: just proved readable for exactly this many bytes.
        std::ptr::read_unaligned(address as *const usize)
    })
}

fn i32_at(address: usize) -> Option<i32> {
    readable(address, size_of::<i32>()).then(|| unsafe {
        // SAFETY: just proved readable for exactly this many bytes.
        std::ptr::read_unaligned(address as *const i32)
    })
}

fn f32_at(address: usize) -> Option<f32> {
    readable(address, size_of::<f32>()).then(|| unsafe {
        // SAFETY: just proved readable for exactly this many bytes.
        std::ptr::read_unaligned(address as *const f32)
    })
}

fn u8_at(address: usize) -> Option<u8> {
    readable(address, 1).then(|| unsafe {
        // SAFETY: just proved readable for exactly this byte.
        std::ptr::read_unaligned(address as *const u8)
    })
}

/// Copies an `EscadraString` (16-byte inline buffer, length `+0x10`, capacity `+0x18`) into `out`.
///
/// The length is checked against the capacity and `out`, so a stale record cannot ask for a large
/// copy, and only ASCII is accepted - every oid and animation name in this game is ASCII.
fn escadra_bytes(address: usize, out: &mut [u8]) -> Option<usize> {
    let length = usize_at(address + 0x10)?;
    let capacity = usize_at(address + 0x18)?;
    if length == 0 || length > capacity || length > out.len() {
        return None;
    }
    let buffer = if capacity >= 0x10 {
        usize_at(address)?
    } else {
        address
    };
    if !readable(buffer, length) {
        return None;
    }
    // SAFETY: the `length` bytes at `buffer` were just proved readable and `out` is at least as
    // long.
    let bytes = unsafe { slice::from_raw_parts(buffer as *const u8, length) };
    if !bytes.iter().all(|byte| *byte < 0x80) {
        return None;
    }
    out[..length].copy_from_slice(bytes);
    Some(length)
}

// ---------------------------------------------------------------------------------------------
// Aircraft types
// ---------------------------------------------------------------------------------------------

/// What one side shows in the panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Totals {
    /// Craft oid -> planes of that type, in the order the panel draws them.
    crafts: Vec<(String, u32)>,
    bombs: i32,
    rockets: i32,
    aam: i32,
    gun: i32,
}

/// The aircraft oids in the table QoL rebuilds from `planes`.
///
/// Cached against the container header: QoL writes a fresh table over the game's at startup, so the
/// sentinel pointer changes exactly when the table does.
fn registered_crafts(game: &Game) -> Arc<Vec<String>> {
    static CACHE: Mutex<Option<(usize, usize, Arc<Vec<String>>)>> = Mutex::new(None);

    let Some(sentinel) = usize_at(game.plane_table) else {
        return Arc::new(Vec::new());
    };
    let size = usize_at(game.plane_table + 8).unwrap_or(0);
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((cached_sentinel, cached_size, crafts)) = &*cache {
        if *cached_sentinel == sentinel && *cached_size == size {
            return crafts.clone();
        }
    }

    let mut crafts = Vec::new();
    if readable(sentinel, TLL_NODE_SIZE) {
        let mut stack = vec![usize_at(sentinel + TLL_PARENT).unwrap_or(sentinel)];
        let mut visited = 0usize;
        while let Some(node) = stack.pop() {
            visited += 1;
            if visited > 256 || node == sentinel || !readable(node, TLL_NODE_SIZE) {
                continue;
            }
            if u8_at(node + TLL_IS_SENTINEL) != Some(0) {
                continue;
            }
            let mut buffer = [0u8; 256];
            if let Some(length) = escadra_bytes(node + TLL_KEY, &mut buffer) {
                if let Ok(name) = std::str::from_utf8(&buffer[..length]) {
                    crafts.push(name.to_owned());
                }
            }
            let left = usize_at(node + TLL_LEFT).unwrap_or(sentinel);
            let right = usize_at(node + TLL_RIGHT).unwrap_or(sentinel);
            if left != sentinel {
                stack.push(left);
            }
            if right != sentinel {
                stack.push(right);
            }
        }
    }
    crafts.sort();
    crafts.dedup();
    let crafts = Arc::new(crafts);
    log::info!(
        "Plane icons: aircraft table holds {} type(s): {}",
        crafts.len(),
        if crafts.is_empty() {
            "-".to_owned()
        } else {
            crafts.join(", ")
        }
    );
    *cache = Some((sentinel, size, crafts.clone()));
    crafts
}

/// Sums one side's roster, or `None` when that side has no roster at all.
///
/// This is the game's own rule: a body counts when it is a `Body`, its `Stats.m_category` has the
/// aircraft bit, and it sits in the side's roster list.
fn scan(game: &Game, roster: Roster, registry: &[String]) -> Option<Totals> {
    let length = usize_at(roster.length)?;
    if length == 0 {
        return None;
    }
    let sentinel = usize_at(roster.list)?;
    if !readable(sentinel, 0x10) {
        return None;
    }
    let part_has_category: FnPartHasCategory = unsafe { std::mem::transmute(game.check_part_flag) };

    let mut totals = Totals::default();
    let mut counts: Vec<(String, u32)> = Vec::new();
    let mut oid_buffer = [0u8; 64];
    let mut node = usize_at(sentinel)?;
    let mut steps = 0usize;
    while node != sentinel {
        steps += 1;
        if steps > 8192 || !readable(node, 0x20) {
            break;
        }
        let body = usize_at(node + NODE_BODY).unwrap_or(0);
        if body != 0
            && readable(body, BODY_SIZE)
            && i32_at(body + BODY_CODE) == Some(CODE_BODY)
            // SAFETY: the body was proved readable; this is the game's own category test.
            && unsafe { part_has_category(body, CATEGORY_AIRCRAFT) }
        {
            if let Some(length) = escadra_bytes(body + BODY_OID, &mut oid_buffer) {
                let oid = &oid_buffer[..length];
                if let Some(entry) = counts.iter_mut().find(|(name, _)| name.as_bytes() == oid) {
                    entry.1 += 1;
                } else if let Some(craft) = registry
                    .iter()
                    .find(|craft| craft.as_bytes() == oid)
                {
                    counts.push((craft.clone(), 1));
                }
            }
            if let Some(logic) = usize_at(body + BODY_AIRCRAFT_LOGIC) {
                if readable(logic, 0x150) {
                    totals.bombs += i32_at(logic + LOGIC_BOMBS).unwrap_or(0);
                    totals.rockets += i32_at(logic + LOGIC_ROCKETS).unwrap_or(0);
                    totals.aam += i32_at(logic + LOGIC_AAM).unwrap_or(0);
                    let rounds = i32_at(logic + LOGIC_GUN_LOAD).unwrap_or(0)
                        + i32_at(logic + LOGIC_GUN_AMMO).unwrap_or(0);
                    totals.gun += rounds * ROUNDS_PER_GUN_AMMO;
                }
            }
        }
        node = usize_at(node).unwrap_or(sentinel);
    }
    totals.crafts = order(counts);
    Some(totals)
}

/// T-7 first, LA-29 second, then everything else alphabetically so the row cannot reshuffle.
fn order(mut crafts: Vec<(String, u32)>) -> Vec<(String, u32)> {
    const VANILLA: [&str; 2] = ["CRAFT_T7", "CRAFT_LA29"];
    crafts.sort_by(|a, b| {
        let rank = |name: &str| VANILLA.iter().position(|vanilla| *vanilla == name).unwrap_or(2);
        rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0))
    });
    crafts
}

/// `CRAFT_SB15` -> `sign_SB15`, the game's own naming for `sign_T7` / `sign_LA29`.
fn sign_name(oid: &str) -> String {
    match oid.strip_prefix("CRAFT_") {
        Some(stem) => format!("sign_{stem}"),
        None => format!("sign_{oid}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// Sprite handles by animation name; `0` records a name the media manager does not have, so a
/// missing `sign_<craft>` is looked up once instead of every frame.
fn sprite(game: &Game, name: &CString) -> usize {
    static SPRITES: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

    let key = name.to_string_lossy().into_owned();
    let mut cache = SPRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, handle)) = cache.iter().find(|(cached, _)| *cached == key) {
        return *handle;
    }
    let media = usize_at(game.media_manager).unwrap_or(0);
    if media == 0 {
        // The media manager is not up yet; do not remember the miss.
        return 0;
    }
    let load: FnLoadMedia = unsafe { std::mem::transmute(game.load_media) };
    // SAFETY: the media manager was readable and `name` is NUL-terminated ASCII.
    let handle = unsafe { load(media, name.as_ptr()) };
    if handle != 0 && readable(handle + SPRITE_BLEND, 4) {
        // The game clears the blend state after each sign load.
        unsafe { std::ptr::write_unaligned((handle + SPRITE_BLEND) as *mut i32, 0) };
    }
    if handle == 0 {
        log::warn!("Plane icons: animation '{key}' is missing; its slot is drawn empty");
    }
    cache.push((key, handle));
    handle
}

/// How wide the game draws `text` in this font.
fn measure(game: &Game, font: usize, text: &CString) -> f32 {
    let measure: FnMeasureString = unsafe { std::mem::transmute(game.measure_string) };
    // SAFETY: the font was created this frame and `text` is NUL-terminated ASCII.
    let width = unsafe { measure(font, text.as_ptr() as *const u8, 0) };
    if width.is_finite() && width >= 0.0 {
        width
    } else {
        0.0
    }
}

/// The scale an icon is drawn at (only ever shrunk into the pixel box) and its drawn half width.
fn icon_fit(sprite: usize) -> (f32, f32) {
    if sprite == 0 {
        return (1.0, 0.0);
    }
    let width = f32_at(sprite + SPRITE_EXTENT_X).unwrap_or(0.0);
    let height = f32_at(sprite + SPRITE_EXTENT_Y).unwrap_or(0.0);
    let scale = if width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite() {
        let fit = (ICON_MAX_WIDTH / width).min(ICON_MAX_HEIGHT / height);
        // Only ever shrink, and only by a sane amount: a bogus extent must not hide an icon.
        if (0.05..1.0).contains(&fit) {
            fit
        } else {
            1.0
        }
    } else {
        1.0
    };
    let half = if width.is_finite() && width > 0.0 {
        width * scale * 0.5
    } else {
        0.0
    };
    (scale, half)
}

fn draw_icon(game: &Game, sprite: usize, scale: f32, colour: u32, x: f32, y: f32) {
    if sprite == 0 {
        return;
    }
    let set_fields: FnSetSpriteFields = unsafe { std::mem::transmute(game.set_sprite_fields) };
    // SAFETY: the handle came from the game's media manager; this is the game's own tint call.
    unsafe { set_fields(sprite, colour, -1) };

    if (scale - 1.0).abs() < 1e-4 {
        let position: FnQuadPosition = unsafe { std::mem::transmute(game.quad_position) };
        // SAFETY: the game's own draw path for a sign sprite.
        unsafe { position(sprite, x, y) };
    } else {
        let render: FnQuadRenderEx = unsafe { std::mem::transmute(game.quad_render_ex) };
        // SAFETY: the same quad, scaled about its cached hotspot.
        unsafe { render(sprite, x, y, 0.0, scale, scale) };
    }
}

fn draw_text(game: &Game, font: usize, x: f32, y: f32, text: &CString) {
    let draw: FnDrawLabel = unsafe { std::mem::transmute(game.draw_label) };
    // SAFETY: the font was created this frame, the text is NUL-terminated ASCII, and the panel's
    // 2D state is bound - the game's own call at this point of the frame.
    unsafe {
        draw(
            font,
            x,
            y,
            0,
            text.as_ptr() as *const u8,
            std::ptr::null_mut(),
            0,
            0.0,
            0.0,
        );
    }
}

fn draw_caption(game: &Game, font: usize, x: f32, y: f32, text: &[u8]) {
    let draw: FnDrawLabel = unsafe { std::mem::transmute(game.draw_label) };
    // SAFETY: the literals are NUL-terminated and the font was created this frame.
    unsafe {
        draw(
            font,
            x,
            y,
            0,
            text.as_ptr(),
            std::ptr::null_mut(),
            0,
            0.0,
            0.0,
        );
    }
}

fn draw_divider(game: &Game, colour: u32, x: f32, y: f32, length: f32) {
    let line: FnDrawWorldLine = unsafe { std::mem::transmute(game.draw_world_line) };
    let from = Vec2::new(x, y);
    let to = Vec2::new(x + length, y);
    // SAFETY: the game's own call shape for this primitive: screen space, solid, 5 units wide.
    unsafe { line(from, to, LINE_WIDTH, colour, 0, 1, 0.0, 1, 1.0) };
}

/// Local positions of one row: `(label_x, icon_x)` per entry, starting at 0.
///
/// This is the game's advance - the icon clears the count by [`ICON_OFFSET`], then the next label
/// starts [`ENTRY_ADVANCE`] after the icon - with [`COLUMN_PITCH`] as a floor so very narrow
/// counts cannot pack two icons into one another.
fn layout_row(widths: &[f32]) -> Vec<(f32, f32)> {
    let mut positions = Vec::with_capacity(widths.len());
    let mut x = 0.0f32;
    for width in widths {
        let icon_x = x + width + ICON_OFFSET;
        positions.push((x, icon_x));
        x = (icon_x + ENTRY_ADVANCE).max(x + COLUMN_PITCH);
    }
    positions
}

/// Right edge of a laid-out row, in the same local units.
fn row_extent(positions: &[(f32, f32)], halves: &[f32]) -> f32 {
    positions
        .iter()
        .zip(halves)
        .map(|((_, icon_x), half)| icon_x + half)
        .fold(0.0, f32::max)
}

/// A side's left edge: its anchor, pulled left when the content would leave the screen.
fn panel_x0(anchor: f32, max_right: f32, content_width: f32) -> f32 {
    (max_right - content_width).min(anchor).max(10.0)
}

/// One entry of a row, resolved before anything is drawn.
struct Entry {
    sprite: usize,
    scale: f32,
    half_width: f32,
    text: CString,
    width: f32,
}

fn entry(game: &Game, count_font: usize, value: i32, sprite: usize) -> Option<Entry> {
    let (scale, half_width) = icon_fit(sprite);
    let text = CString::new(value.to_string()).ok()?;
    let width = measure(game, count_font, &text);
    Some(Entry {
        sprite,
        scale,
        half_width,
        text,
        width,
    })
}

fn draw_side(
    game: &Game,
    anchor: f32,
    max_right: f32,
    colour: u32,
    count_font: usize,
    caption_font: usize,
    totals: &Totals,
) {
    let set_colour: FnSetFontColor = unsafe { std::mem::transmute(game.font_set_color) };
    // SAFETY: both fonts were created this frame; the game recolours them the same way.
    unsafe {
        set_colour(count_font, colour);
        set_colour(caption_font, colour);
    }

    let visible = COLUMNS * MAX_ROWS;
    if totals.crafts.len() > visible {
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::warn!(
                "Plane icons: {} aircraft types are present but only {visible} fit ({COLUMNS} x \
                 {MAX_ROWS}); the rest are hidden",
                totals.crafts.len()
            );
        }
    }
    let types = &totals.crafts[..totals.crafts.len().min(visible)];

    // Resolve both row groups first: the panel's left edge depends on the widest row.
    let mut craft_rows: Vec<Vec<Entry>> = Vec::new();
    for chunk in types.chunks(COLUMNS) {
        let mut row = Vec::with_capacity(chunk.len());
        for (oid, count) in chunk {
            let Ok(name) = CString::new(sign_name(oid)) else {
                continue;
            };
            let handle = sprite(game, &name);
            if let Some(item) = entry(game, count_font, *count as i32, handle) {
                row.push(item);
            }
        }
        craft_rows.push(row);
    }
    let rows_used = craft_rows.len().max(1);
    let shift = (rows_used - 1) as f32 * ROW_PITCH;

    let mut munitions: Vec<Entry> = Vec::new();
    for (name, value) in MUNITIONS {
        let count = value(totals);
        if count == 0 {
            continue;
        }
        let Ok(name) = CString::new(name) else {
            continue;
        };
        let handle = sprite(game, &name);
        if let Some(item) = entry(game, count_font, count, handle) {
            munitions.push(item);
        }
    }

    // Lay both groups out locally, then pick the left edge from the widest of them.
    let mut content_width = LINE_LENGTH;
    let mut craft_positions: Vec<Vec<(f32, f32)>> = Vec::new();
    for row in &craft_rows {
        let widths: Vec<f32> = row.iter().map(|item| item.width).collect();
        let halves: Vec<f32> = row.iter().map(|item| item.half_width).collect();
        let positions = layout_row(&widths);
        content_width = content_width.max(row_extent(&positions, &halves) + DIVIDER_PAD);
        craft_positions.push(positions);
    }
    let munition_widths: Vec<f32> = munitions.iter().map(|item| item.width).collect();
    let munition_halves: Vec<f32> = munitions.iter().map(|item| item.half_width).collect();
    let munition_positions = layout_row(&munition_widths);
    content_width = content_width.max(row_extent(&munition_positions, &munition_halves) + DIVIDER_PAD);

    let x0 = panel_x0(anchor, max_right, content_width);

    // Aircraft rows.
    for (row_index, row) in craft_rows.iter().enumerate() {
        let label_y = LABEL_Y + row_index as f32 * ROW_PITCH;
        let icon_y = ICON_Y + row_index as f32 * ROW_PITCH;
        for (item, (label_x, icon_x)) in row.iter().zip(&craft_positions[row_index]) {
            draw_text(game, count_font, x0 + label_x, label_y, &item.text);
            draw_icon(game, item.sprite, item.scale, colour, x0 + icon_x, icon_y);
        }
    }

    draw_caption(game, caption_font, x0, CRAFTS_CAPTION_Y + shift, b"CRAFTS\0");
    draw_divider(game, colour, x0, LINE_Y + shift, content_width);
    draw_caption(game, caption_font, x0, MUNITION_CAPTION_Y + shift, b"MUNITION\0");

    // Munitions row.
    for (item, (label_x, icon_x)) in munitions.iter().zip(&munition_positions) {
        draw_text(game, count_font, x0 + label_x, MUNITION_LABEL_Y + shift, &item.text);
        draw_icon(
            game,
            item.sprite,
            item.scale,
            colour,
            x0 + icon_x,
            MUNITION_ICON_Y + shift,
        );
    }
}

/// The whole panel: body markers first (the game does), then both sides.
///
/// # Safety
///
/// Runs inside the game's battle HUD pass, from the trampoline that replaced the panel call, with
/// the frame's 2D state already bound.
unsafe fn draw(game: &Game) {
    let body_markers: FnBodyMarkers = unsafe { std::mem::transmute(game.body_markers) };
    // SAFETY: the game calls this as the first statement of the function this hook replaces.
    unsafe { body_markers() };

    let registry = registered_crafts(game);
    let player = scan(game, game.player_roster, &registry);
    let enemy = scan(game, game.enemy_roster, &registry);
    if player.is_none() && enemy.is_none() {
        // Neither side has a battle roster: the game draws nothing either.
        return;
    }

    let setup: FnSetupFont = unsafe { std::mem::transmute(game.font_setup) };
    // SAFETY: the game's own font creation, with the same names and scale.
    let count_font = unsafe { setup(FONT_COUNT.as_ptr() as *const c_char, 1.0, 0, FONT_COLOR) };
    let caption_font = unsafe { setup(FONT_CAPTION.as_ptr() as *const c_char, 1.0, 0, FONT_COLOR) };
    if count_font == 0 || caption_font == 0 {
        return;
    }

    let width = i32_at(game.graphics_width).unwrap_or(FALLBACK_WIDTH);
    let width = if (100..=8192).contains(&width) {
        width as f32
    } else {
        FALLBACK_WIDTH as f32
    };

    if let Some(player) = &player {
        draw_side(
            game,
            PLAYER_X,
            width - EDGE_INSET,
            PLAYER_COLOR,
            count_font,
            caption_font,
            player,
        );
    }
    if let Some(enemy) = &enemy {
        draw_side(
            game,
            width - ENEMY_INSET,
            width - EDGE_INSET,
            ENEMY_COLOR,
            count_font,
            caption_font,
            enemy,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_patch_site_is_the_panel_call() {
        assert_eq!(SITE_RVA + STOLEN.len(), CONTINUATION_RVA);
        assert_eq!(STOLEN[0], 0xe8);
        let relative = i32::from_le_bytes(STOLEN[1..].try_into().unwrap()) as isize;
        assert_eq!((SITE_RVA + 5) as isize + relative, TARGET_RVA as isize);
    }

    #[test]
    fn the_trampoline_calls_the_hook_and_resumes_after_the_call() {
        const BASE: usize = 0x180000000;
        let continuation = BASE + CONTINUATION_RVA;
        let code = build_trampoline(continuation).build(BASE).unwrap();

        assert_eq!(&code[..2], &[0xff, 0x15], "CALL [RIP+2]");
        let hook = (draw_hook as *const () as usize).to_le_bytes();
        assert!(code.windows(8).any(|window| window == hook), "hook address");
        // A mid-function call site is already 0 mod 16: no stack adjustment may be inserted.
        assert!(!code
            .windows(4)
            .any(|window| window == [0x48, 0x83, 0xec, 0x08]));

        let jump_at = code.len() - 5;
        assert_eq!(code[jump_at], 0xe9);
        let displacement = i32::from_le_bytes(code[jump_at + 1..].try_into().unwrap());
        let next = (BASE + jump_at + 5) as isize;
        assert_eq!(next + displacement as isize, continuation as isize);
    }

    #[test]
    fn a_row_follows_the_game_advance_then_keeps_the_pitch_floor() {
        let positions = layout_row(&[21.0, 21.0, 21.0]);
        assert_eq!(positions[0], (0.0, 56.0));
        assert_eq!(positions[1], (106.0, 162.0));
        assert_eq!(positions[2], (212.0, 268.0));

        // Zero-width counts would put the next label at +50; the floor holds it at +85.
        assert_eq!(layout_row(&[0.0, 0.0])[1].0, COLUMN_PITCH);
    }

    #[test]
    fn the_row_extent_is_the_last_icon_edge() {
        let positions = layout_row(&[21.0, 21.0]);
        assert_eq!(row_extent(&positions, &[29.0, 29.0]), 191.0);
        assert_eq!(row_extent(&positions, &[29.0, 24.0]), 186.0);
    }

    #[test]
    fn the_enemy_panel_only_moves_left_when_it_has_to() {
        assert_eq!(panel_x0(1520.0, 1890.0, 240.0), 1520.0);
        assert_eq!(panel_x0(1520.0, 1890.0, 600.0), 1290.0);
        assert_eq!(panel_x0(30.0, 1890.0, 600.0), 30.0);
        assert_eq!(panel_x0(30.0, 100.0, 400.0), 10.0);
    }

    #[test]
    fn sign_names_follow_the_game() {
        assert_eq!(sign_name("CRAFT_T7"), "sign_T7");
        assert_eq!(sign_name("CRAFT_SB15"), "sign_SB15");
        assert_eq!(sign_name("MYPLANE"), "sign_MYPLANE");
    }

    #[test]
    fn vanilla_types_come_first_then_alphabetical() {
        let ordered = order(vec![
            ("CRAFT_SB15".to_owned(), 1),
            ("CRAFT_T7".to_owned(), 2),
            ("CRAFT_MB110".to_owned(), 3),
            ("CRAFT_LA29".to_owned(), 4),
        ]);
        assert_eq!(
            ordered
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["CRAFT_T7", "CRAFT_LA29", "CRAFT_MB110", "CRAFT_SB15"]
        );
    }
}
