//! Reworks the battle **CRAFTS / MUNITION** panel so every aircraft type is listed, not just the
//! T-7 and LA-29.
//!
//! The panel is drawn once per frame inside `combatEvents`: `somethingAircraftDisplay__`
//! (`0x140278780`) on 1.151 and `Hud_DrawMapSignLabels` (`0x14029b3e0`) on 1.163.
//! Its aircraft list is not data: the oid of every part is compared against two hardcoded strings
//! (`"CRAFT_LA29"` and `"CRAFT_T7"`), each feeding its own counter and its own draw block.
//! A third type - for example a QoL aircraft such as `CRAFT_SB15` - is therefore never counted and
//! never gets an icon.
//!
//! This module replaces that call with the same drawing driven by a list:
//!
//! - a type is shown when its oid is a key of the aircraft table rebuilt from `planes`.
//! - the icon is the `sign_<aircraft name>` animation, tinted with the side colour.
//! - a missing animation keeps the count and leaves the icon slot empty, so the row never shifts.
//! - three types fit per row, and a second row moves the captions, the divider and the munitions
//!   row down by one row pitch.
//! - counts and munitions are the game's own numbers, read from the same fields its own panel uses.
//!
//! The geometry, colours and draw calls are the game's own.
//! The single addition is that the enemy panel's left edge is pulled left when its widest row would
//! run past the screen, because both rows grow rightwards and the enemy panel only has 400 units.

use std::ffi::{c_char, c_void, CString};
use std::sync::{Arc, Mutex, OnceLock};

use highfleet::general::{EscadraString, Link, SentinelList, TllContainer};
#[cfg(feature = "1_151")]
use highfleet::v1_151::Body;
#[cfg(not(feature = "1_151"))]
use highfleet::v1_163::Body;
use patchy::{PatchSession, ProcessModule, Result, Trampoline};

use crate::structs::aircraft_logic::AircraftLogic;
use crate::structs::plane::Plane;
use crate::structs::sign_sprite::SignSprite;

/// The `CALL` to the panel inside `combatEvents`, relative to the module base.
#[cfg(feature = "1_151")]
const SITE_RVA: usize = 0x277bad;
#[cfg(not(feature = "1_151"))]
const SITE_RVA: usize = 0x29a8f6;

/// The panel function the call reaches, relative to the module base.
#[cfg(feature = "1_151")]
const TARGET_RVA: usize = 0x278780;
#[cfg(not(feature = "1_151"))]
const TARGET_RVA: usize = 0x29b3e0;

/// The instruction after the call, where the trampoline resumes.
#[cfg(feature = "1_151")]
const CONTINUATION_RVA: usize = 0x277bb2;
#[cfg(not(feature = "1_151"))]
const CONTINUATION_RVA: usize = 0x29a8fb;

/// A near `CALL rel32` is five bytes.
const PANEL_CALL_LEN: usize = 5;

// Game functions this module calls, relative to the module base.
#[cfg(feature = "1_151")]
const BODY_MARKERS_RVA: usize = 0x278120;
#[cfg(not(feature = "1_151"))]
const BODY_MARKERS_RVA: usize = 0x29ae40;

#[cfg(feature = "1_151")]
const LOAD_MEDIA_RVA: usize = 0x164740;
#[cfg(not(feature = "1_151"))]
const LOAD_MEDIA_RVA: usize = 0x170230;

#[cfg(feature = "1_151")]
const SET_SPRITE_FIELDS_RVA: usize = 0x166ff0;
#[cfg(not(feature = "1_151"))]
const SET_SPRITE_FIELDS_RVA: usize = 0x172ae0;

#[cfg(feature = "1_151")]
const SPRITE_POSITION_RVA: usize = 0x166ad0;
#[cfg(not(feature = "1_151"))]
const SPRITE_POSITION_RVA: usize = 0x1725c0;

#[cfg(feature = "1_151")]
const SPRITE_RENDER_EX_RVA: usize = 0x166fb0;
#[cfg(not(feature = "1_151"))]
const SPRITE_RENDER_EX_RVA: usize = 0x172aa0;

#[cfg(feature = "1_151")]
const FONT_SETUP_RVA: usize = 0x198e70;
#[cfg(not(feature = "1_151"))]
const FONT_SETUP_RVA: usize = 0x1a8710;

#[cfg(feature = "1_151")]
const FONT_SET_COLOR_RVA: usize = 0x163630;
#[cfg(not(feature = "1_151"))]
const FONT_SET_COLOR_RVA: usize = 0x16f120;

#[cfg(feature = "1_151")]
const MEASURE_STRING_RVA: usize = 0x162f00;
#[cfg(not(feature = "1_151"))]
const MEASURE_STRING_RVA: usize = 0x16e9f0;

#[cfg(feature = "1_151")]
const DRAW_LABEL_RVA: usize = 0x163050;
#[cfg(not(feature = "1_151"))]
const DRAW_LABEL_RVA: usize = 0x16eb40;

#[cfg(feature = "1_151")]
const DRAW_WORLD_LINE_RVA: usize = 0x2279a0;
#[cfg(not(feature = "1_151"))]
const DRAW_WORLD_LINE_RVA: usize = 0x248ba0;

#[cfg(feature = "1_151")]
const PART_HAS_CATEGORY_RVA: usize = 0x47960;
#[cfg(not(feature = "1_151"))]
const PART_HAS_CATEGORY_RVA: usize = 0x49470;

// Game globals this module reads, relative to the module base.
#[cfg(feature = "1_151")]
const MEDIA_MANAGER_RVA: usize = 0x3921f78;
#[cfg(not(feature = "1_151"))]
const MEDIA_MANAGER_RVA: usize = 0x3a119e8;

#[cfg(feature = "1_151")]
const GRAPHICS_WIDTH_RVA: usize = 0x39443dc;
#[cfg(not(feature = "1_151"))]
const GRAPHICS_WIDTH_RVA: usize = 0x3a158fc;

#[cfg(feature = "1_151")]
const PLANE_TABLE_RVA: usize = 0x3942740;
#[cfg(not(feature = "1_151"))]
const PLANE_TABLE_RVA: usize = 0x3a13c50;

/// Player roster header: `SentinelList<Link>` with the side's own parts.
#[cfg(feature = "1_151")]
const PLAYER_ROSTER_RVA: usize = 0x7eedd78;
#[cfg(not(feature = "1_151"))]
const PLAYER_ROSTER_RVA: usize = 0x7fc73b0;

/// Enemy roster header.
#[cfg(feature = "1_151")]
const ENEMY_ROSTER_RVA: usize = 0x7eedec0;
#[cfg(not(feature = "1_151"))]
const ENEMY_ROSTER_RVA: usize = 0x7fc75a8;

/// `Body` class code, the same value every serialised body carries.
const CODE_BODY: u64 = 15;
/// `Stats.m_category` bit that marks the aircraft category.
const CATEGORY_AIRCRAFT: u32 = 0x400;
/// Rounds per gun-ammo unit, the multiplier the game's own panel applies.
const ROUNDS_PER_GUN_AMMO: i32 = 20;

// The panel's own geometry, all of it the game's.
const PLAYER_X: f32 = 30.0;
const ENEMY_INSET: f32 = 400.0;
const EDGE_INSET: f32 = 30.0;
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
/// How far past the last icon the divider runs.
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

const PLAYER_COLOR: u32 = 0xFF00_E4FF;
const ENEMY_COLOR: u32 = 0xFFA8_FC00;

const FONT_COUNT: &[u8] = b"pollock_60\0";
const FONT_CAPTION: &[u8] = b"dinpro_14_reg\0";

type FnBodyMarkers = unsafe extern "C" fn();
type FnLoadMedia = unsafe extern "C" fn(*const c_void, *const c_char) -> *mut SignSprite;
type FnSetSpriteFields = unsafe extern "C" fn(*mut SignSprite, u32, i32);
type FnSpritePosition = unsafe extern "C" fn(*mut SignSprite, f32, f32);
type FnSpriteRenderEx = unsafe extern "C" fn(*mut SignSprite, f32, f32, f32, f32, f32);
type FnSetupFont = unsafe extern "C" fn(*const c_char, f32, i32, u32) -> *mut c_void;
type FnSetFontColor = unsafe extern "C" fn(*mut c_void, u32);
type FnMeasureString = unsafe extern "C" fn(*mut c_void, *const u8, i32) -> f32;
type FnDrawLabel = unsafe extern "C" fn(
    *mut c_void,
    f32,
    f32,
    u32,
    *const u8,
    *mut f32,
    *const c_void,
    f32,
    f32,
) -> i32;
type FnDrawWorldLine = unsafe extern "C" fn(Vec2, Vec2, f32, u32, u32, u8, f32, u32, f32);
type FnPartHasCategory = unsafe extern "C" fn(*mut Body, u32) -> bool;

/// A 2D point by value: `Gfx_DrawWorldLine` takes two of them in `RCX` and `RDX`.
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
///
/// 1.151 has no gun entry in this panel at all: the game only counts bombs, rockets and K-13.
#[cfg(feature = "1_151")]
const MUNITIONS: [(&str, fn(&Totals) -> i32); 3] = [
    ("sign_GPB", |totals| totals.bombs),
    ("sign_NAR", |totals| totals.rockets),
    ("sign_AAM", |totals| totals.aam),
];
#[cfg(not(feature = "1_151"))]
const MUNITIONS: [(&str, fn(&Totals) -> i32); 4] = [
    ("sign_GPB", |totals| totals.bombs),
    ("sign_NAR", |totals| totals.rockets),
    ("sign_AAM", |totals| totals.aam),
    ("sign_GUN", |totals| totals.gun),
];

/// 1.151 always draws the T-7 and LA-29 icons, even with no planes of that type.
/// 1.163 hides a zero count instead.
const ALWAYS_SHOW_VANILLA_CRAFTS: bool = cfg!(feature = "1_151");
/// 1.151 draws all three munition entries unconditionally, while 1.163 skips a zero.
const HIDE_ZERO_MUNITIONS: bool = cfg!(not(feature = "1_151"));

/// What one side shows in the panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Totals {
    /// Craft oid -> planes of that type, in the order the panel draws them.
    crafts: Vec<(String, u32)>,
    bombs: i32,
    rockets: i32,
    aam: i32,
    /// Only the 1.163 panel has a gun entry, so 1.151 never reads this.
    #[cfg_attr(feature = "1_151", allow(dead_code))]
    gun: i32,
}

/// Runtime addresses, resolved once when the patch is prepared.
struct Game {
    base: usize,
    body_markers: usize,
    load_media: usize,
    set_sprite_fields: usize,
    sprite_position: usize,
    sprite_render_ex: usize,
    font_setup: usize,
    font_set_color: usize,
    measure_string: usize,
    draw_label: usize,
    draw_world_line: usize,
    part_has_category: usize,
}

impl Game {
    fn new(module: ProcessModule) -> Result<Self> {
        Ok(Self {
            base: module.base_address(),
            body_markers: module.resolve_rva(BODY_MARKERS_RVA)?,
            load_media: module.resolve_rva(LOAD_MEDIA_RVA)?,
            set_sprite_fields: module.resolve_rva(SET_SPRITE_FIELDS_RVA)?,
            sprite_position: module.resolve_rva(SPRITE_POSITION_RVA)?,
            sprite_render_ex: module.resolve_rva(SPRITE_RENDER_EX_RVA)?,
            font_setup: module.resolve_rva(FONT_SETUP_RVA)?,
            font_set_color: module.resolve_rva(FONT_SET_COLOR_RVA)?,
            measure_string: module.resolve_rva(MEASURE_STRING_RVA)?,
            draw_label: module.resolve_rva(DRAW_LABEL_RVA)?,
            draw_world_line: module.resolve_rva(DRAW_WORLD_LINE_RVA)?,
            part_has_category: module.resolve_rva(PART_HAS_CATEGORY_RVA)?,
        })
    }

    /// A game global by its RVA.
    fn global<T>(&self, rva: usize) -> *mut T {
        (self.base + rva) as *mut T
    }
}

static GAME: OnceLock<Game> = OnceLock::new();

/// Replaces the battle CRAFTS / MUNITION panel so every aircraft in `planes` gets an icon.
///
/// # Safety
///
/// Installs a detour into the running game's `.text`; it must only be called from the mod loader's
/// `init`, exactly like the other patches in this crate.
pub unsafe fn patch_plane_icons(session: &mut PatchSession) -> Result {
    let module = ProcessModule::main()?;
    let site = module.resolve_rva(SITE_RVA)?;
    let continuation = module.resolve_rva(CONTINUATION_RVA)?;
    let panel = module.resolve_rva(TARGET_RVA)?;

    let _ = GAME.set(Game::new(module)?);
    session.detour_trampoline(site, PANEL_CALL_LEN, build_trampoline(continuation))?;

    log::info!("Prepared plane icons rework (panel {panel:#x})");
    Ok(())
}

fn build_trampoline(continuation: usize) -> Trampoline {
    let mut trampoline = Trampoline::new();
    // The replaced instruction is a mid-function call, where the stack is already 16-byte aligned.
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
// Aircraft types
// ---------------------------------------------------------------------------------------------

/// The aircraft oids in the table rebuilt from `planes`.
///
/// Cached against the container header: the table is rebuilt once at startup, so the sentinel
/// pointer and length change exactly when the table does.
fn registered_crafts(game: &Game) -> Arc<Vec<String>> {
    static CACHE: Mutex<Option<(usize, usize, Arc<Vec<String>>)>> = Mutex::new(None);

    // SAFETY: the table is the game's own container, rebuilt by `patch_planes` before any battle.
    let table = unsafe { &mut *game.global::<TllContainer<EscadraString, Plane>>(PLANE_TABLE_RVA) };
    let (sentinel, length) = (table.sentinel() as usize, table.len());

    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((cached_sentinel, cached_length, crafts)) = &*cache {
        if *cached_sentinel == sentinel && *cached_length == length {
            return crafts.clone();
        }
    }

    let mut crafts: Vec<String> = table
        .get_map()
        .keys()
        .map(|key| key.get_string().to_owned())
        .collect();
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
    *cache = Some((sentinel, length, crafts.clone()));
    crafts
}

/// Sums one side's roster, or `None` when that side has no roster at all.
///
/// This is the game's own rule: a part counts when its body carries the `Body` code and its
/// `Stats.m_category` has the aircraft bit.
fn scan(game: &Game, roster_rva: usize, registry: &[String]) -> Option<Totals> {
    let part_has_category: FnPartHasCategory =
        unsafe { std::mem::transmute(game.part_has_category) };

    // SAFETY: the level scripts initialise this roster header before the battle HUD draws.
    let roster = unsafe { &*game.global::<SentinelList<Link>>(roster_rva) };
    if roster.length == 0 {
        return None;
    }

    let head = roster.head;
    let mut totals = Totals::default();
    let mut counts: Vec<(String, u32)> = Vec::new();

    // SAFETY: `head` is the sentinel of a live circular list, so its next pointer is a valid node.
    let mut node = unsafe { (*head).next };
    while node != head {
        // SAFETY: every non-sentinel node of this list holds a link to one part.
        let body = unsafe { (*node).value.target as *mut Body };
        // SAFETY: `body` is the linked part, or null for an empty entry.
        let is_aircraft = !body.is_null()
            && unsafe { (*body).base.base.base.code } == CODE_BODY
            && unsafe { part_has_category(body, CATEGORY_AIRCRAFT) };
        if is_aircraft {
            let oid = unsafe { (*body).oid.get_string() };
            match counts.iter_mut().find(|(name, _)| name.as_str() == oid) {
                Some((_, count)) => *count += 1,
                None => {
                    if let Some(craft) = registry.iter().find(|craft| craft.as_str() == oid) {
                        counts.push((craft.to_string(), 1));
                    }
                }
            }

            // SAFETY: the body owns its aircraft-logic record while the aircraft exists.
            let logic = unsafe { (*body).aircraft_logic as *mut AircraftLogic };
            if !logic.is_null() {
                let logic = unsafe { &*logic };
                totals.bombs += logic.bombs.count;
                totals.rockets += logic.rockets.count;
                totals.aam += logic.aa_missiles.count;
                let rounds = logic.gun_load + logic.gun_reserve_ammo;
                totals.gun += rounds * ROUNDS_PER_GUN_AMMO;
            }
        }

        // SAFETY: the list is circular, so `next` is always a node of this list.
        node = unsafe { (*node).next };
    }

    totals.crafts = order(counts);
    if ALWAYS_SHOW_VANILLA_CRAFTS {
        // 1.151 draws both vanilla icons unconditionally, so keep them even with zero planes.
        for vanilla in ["CRAFT_T7", "CRAFT_LA29"] {
            if registry.iter().any(|craft| craft == vanilla)
                && !totals.crafts.iter().any(|(name, _)| name == vanilla)
            {
                totals.crafts.push((vanilla.to_owned(), 0));
            }
        }
        totals.crafts = order(totals.crafts);
    }
    Some(totals)
}

/// T-7 first, LA-29 second, then everything else alphabetically so the row cannot reshuffle.
fn order(mut crafts: Vec<(String, u32)>) -> Vec<(String, u32)> {
    const VANILLA: [&str; 2] = ["CRAFT_T7", "CRAFT_LA29"];
    crafts.sort_by(|a, b| {
        let rank = |name: &str| {
            VANILLA
                .iter()
                .position(|vanilla| *vanilla == name)
                .unwrap_or(2)
        };
        rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0))
    });
    crafts
}

/// `CRAFT_SB15` -> `sign_SB15`, the game's own naming for `sign_T7` and `sign_LA29`.
fn sign_name(oid: &str) -> String {
    match oid.strip_prefix("CRAFT_") {
        Some(stem) => format!("sign_{stem}"),
        None => format!("sign_{oid}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// Loads a sign animation, or returns null when the media manager does not have it.
///
/// Handles are cached by animation name, so a missing icon is reported once instead of every frame.
fn sprite(game: &Game, name: &CString) -> *mut SignSprite {
    static SPRITES: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

    let key = name.to_string_lossy().into_owned();
    let mut cache = SPRITES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, handle)) = cache.iter().find(|(cached, _)| *cached == key) {
        return *handle as *mut SignSprite;
    }

    // SAFETY: the media manager global holds the manager the game passes to this call itself.
    let media = unsafe { *game.global::<*const c_void>(MEDIA_MANAGER_RVA) };
    let load: FnLoadMedia = unsafe { std::mem::transmute(game.load_media) };
    // SAFETY: `name` is NUL-terminated ASCII and `media` is the live manager.
    let handle = unsafe { load(media, name.as_ptr()) };
    if handle.is_null() {
        log::warn!("Plane icons: animation '{key}' is missing; its slot is drawn empty");
    } else {
        // SAFETY: a freshly loaded animation is this module's to configure, as it is the game's.
        unsafe { (*handle).blend_state = 0 };
    }

    cache.push((key, handle as usize));
    handle
}

/// How wide the game draws `text` in this font.
fn measure(game: &Game, font: *mut c_void, text: &CString) -> f32 {
    let measure: FnMeasureString = unsafe { std::mem::transmute(game.measure_string) };
    // SAFETY: the font was created this frame and `text` is NUL-terminated ASCII.
    let width = unsafe { measure(font, text.as_ptr() as *const u8, 0) };
    if width > 0.0 {
        width
    } else {
        0.0
    }
}

/// The scale an icon is drawn at, and its drawn half width.
///
/// Art that already fits the pixel box is drawn at its own size.
fn icon_fit(sprite: *mut SignSprite) -> (f32, f32) {
    if sprite.is_null() {
        return (1.0, 0.0);
    }

    // SAFETY: the handle came from the media manager and is live for the whole battle.
    let (width, height) = unsafe { ((*sprite).extent_x, (*sprite).extent_y) };
    let fit = (ICON_MAX_WIDTH / width).min(ICON_MAX_HEIGHT / height);
    let scale = if (0.05..1.0).contains(&fit) { fit } else { 1.0 };

    (scale, width * scale * 0.5)
}

fn draw_icon(game: &Game, sprite: *mut SignSprite, scale: f32, colour: u32, x: f32, y: f32) {
    if sprite.is_null() {
        return;
    }

    let set_fields: FnSetSpriteFields = unsafe { std::mem::transmute(game.set_sprite_fields) };
    // SAFETY: the handle came from the media manager; this is the game's own tint call.
    unsafe { set_fields(sprite, colour, -1) };

    if (scale - 1.0).abs() < 1e-4 {
        let position: FnSpritePosition = unsafe { std::mem::transmute(game.sprite_position) };
        // SAFETY: the game's own draw path for a sign sprite.
        unsafe { position(sprite, x, y) };
    } else {
        let render: FnSpriteRenderEx = unsafe { std::mem::transmute(game.sprite_render_ex) };
        // SAFETY: the same quad, scaled about its cached hotspot.
        unsafe { render(sprite, x, y, 0.0, scale, scale) };
    }
}

fn draw_text(game: &Game, font: *mut c_void, x: f32, y: f32, text: &CString) {
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
            std::ptr::null(),
            0.0,
            0.0,
        );
    }
}

fn draw_caption(game: &Game, font: *mut c_void, x: f32, y: f32, text: &[u8]) {
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
            std::ptr::null(),
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
/// starts [`ENTRY_ADVANCE`] after the icon - with [`COLUMN_PITCH`] as a floor so very narrow counts
/// cannot pack two icons into one another.
fn layout_row(widths: &[f32]) -> Vec<(f32, f32)> {
    let mut positions = Vec::with_capacity(widths.len());
    let mut x = 0.0;

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
    (max_right - content_width).min(anchor)
}

/// One entry of a row, resolved before anything is drawn.
struct Entry {
    sprite: *mut SignSprite,
    scale: f32,
    half_width: f32,
    text: CString,
    width: f32,
}

fn entry(
    game: &Game,
    count_font: *mut c_void,
    value: i32,
    sprite: *mut SignSprite,
) -> Option<Entry> {
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

/// Lays out one group of entries and reports the widest row edge.
fn place_rows(
    game: &Game,
    count_font: *mut c_void,
    values: &[i32],
    sprites: &[*mut SignSprite],
    columns: usize,
) -> (Vec<Vec<Entry>>, Vec<Vec<(f32, f32)>>, f32) {
    let mut rows: Vec<Vec<Entry>> = Vec::new();
    let mut positions: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut extent = 0.0f32;

    for (values, sprites) in values.chunks(columns).zip(sprites.chunks(columns)) {
        let row: Vec<Entry> = values
            .iter()
            .zip(sprites)
            .filter_map(|(value, sprite)| entry(game, count_font, *value, *sprite))
            .collect();

        let widths: Vec<f32> = row.iter().map(|item| item.width).collect();
        let halves: Vec<f32> = row.iter().map(|item| item.half_width).collect();
        let row_positions = layout_row(&widths);
        extent = extent.max(row_extent(&row_positions, &halves) + DIVIDER_PAD);

        rows.push(row);
        positions.push(row_positions);
    }

    (rows, positions, extent)
}

fn draw_side(
    game: &Game,
    anchor: f32,
    max_right: f32,
    colour: u32,
    count_font: *mut c_void,
    caption_font: *mut c_void,
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

    // Resolve the ids and counts first: the panel's left edge depends on the widest row.
    let mut craft_values: Vec<i32> = Vec::new();
    let mut craft_sprites: Vec<*mut SignSprite> = Vec::new();
    for (oid, count) in totals.crafts.iter().take(visible) {
        let Ok(name) = CString::new(sign_name(oid)) else {
            continue;
        };
        craft_values.push(*count as i32);
        craft_sprites.push(sprite(game, &name));
    }

    let mut munition_values: Vec<i32> = Vec::new();
    let mut munition_sprites: Vec<*mut SignSprite> = Vec::new();
    for (name, value) in MUNITIONS {
        let count = value(totals);
        if count == 0 && HIDE_ZERO_MUNITIONS {
            continue;
        }
        let Ok(name) = CString::new(name) else {
            continue;
        };
        munition_values.push(count);
        munition_sprites.push(sprite(game, &name));
    }

    let (craft_rows, craft_positions, craft_extent) =
        place_rows(game, count_font, &craft_values, &craft_sprites, COLUMNS);
    let (munition_rows, munition_positions, munition_extent) = place_rows(
        game,
        count_font,
        &munition_values,
        &munition_sprites,
        MUNITIONS.len(),
    );

    let content_width = LINE_LENGTH.max(craft_extent).max(munition_extent);
    let x0 = panel_x0(anchor, max_right, content_width);
    let rows_used = craft_rows.len().max(1);
    let shift = (rows_used - 1) as f32 * ROW_PITCH;

    // Aircraft rows.
    for (row_index, (row, positions)) in craft_rows.iter().zip(&craft_positions).enumerate() {
        let label_y = LABEL_Y + row_index as f32 * ROW_PITCH;
        let icon_y = ICON_Y + row_index as f32 * ROW_PITCH;

        for (item, (label_x, icon_x)) in row.iter().zip(positions) {
            draw_text(game, count_font, x0 + label_x, label_y, &item.text);
            draw_icon(game, item.sprite, item.scale, colour, x0 + icon_x, icon_y);
        }
    }

    draw_caption(
        game,
        caption_font,
        x0,
        CRAFTS_CAPTION_Y + shift,
        b"CRAFTS\0",
    );
    draw_divider(game, colour, x0, LINE_Y + shift, content_width);
    draw_caption(
        game,
        caption_font,
        x0,
        MUNITION_CAPTION_Y + shift,
        b"MUNITION\0",
    );

    // Munitions row: a single row, laid out from the same left edge.
    if let (Some(row), Some(positions)) = (munition_rows.first(), munition_positions.first()) {
        for (item, (label_x, icon_x)) in row.iter().zip(positions) {
            draw_text(
                game,
                count_font,
                x0 + label_x,
                MUNITION_LABEL_Y + shift,
                &item.text,
            );
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
}

/// The whole panel: body markers first, as the game does, then both sides.
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
    let player = scan(game, PLAYER_ROSTER_RVA, &registry);
    let enemy = scan(game, ENEMY_ROSTER_RVA, &registry);
    if player.is_none() && enemy.is_none() {
        // Neither side has a battle roster: the game draws nothing either.
        return;
    }

    let setup: FnSetupFont = unsafe { std::mem::transmute(game.font_setup) };
    // SAFETY: the game's own font creation, with the same names and scale.
    let count_font = unsafe { setup(FONT_COUNT.as_ptr() as *const c_char, 1.0, 0, ENEMY_COLOR) };
    let caption_font =
        unsafe { setup(FONT_CAPTION.as_ptr() as *const c_char, 1.0, 0, ENEMY_COLOR) };

    // SAFETY: the video task sets this before any battle HUD draw.
    let width = unsafe { *game.global::<i32>(GRAPHICS_WIDTH_RVA) } as f32;

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
        assert_eq!(SITE_RVA + PANEL_CALL_LEN, CONTINUATION_RVA);
        assert_eq!(TARGET_RVA > CONTINUATION_RVA, true);
    }

    #[test]
    fn the_trampoline_calls_the_hook_and_resumes_after_the_call() {
        const BASE: usize = 0x180000000;
        let continuation = BASE + CONTINUATION_RVA;
        let code = build_trampoline(continuation).build(BASE).unwrap();

        assert_eq!(&code[..2], &[0xff, 0x15], "CALL [RIP+2]");
        let hook = (draw_hook as *const () as usize).to_le_bytes();
        assert!(code.windows(8).any(|window| window == hook), "hook address");
        // A mid-function call site is already 16-byte aligned: no stack adjustment may be inserted.
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
