//! Custom part menus: shop bars that open a list of configured parts.
//!
//! Vanilla city shops sell a handful of resource bars and push every part without a resource into the shop as its own row.
//! This module adds a bar with its own stock whose flyout is built from a private library node.
//!
//! # Hooks
//!
//! `inject_menus` runs from the shop-generation hook in [`crate::parts`] and pushes one bar per matching rule into the city shop node.
//! `hover` wraps the `BottomMenu_` call and maps a custom bar oid to the menu node for one call.
//! `click` wraps `ClickFunc` in the editor and stops the bar from being attached.
//! Every gated part gets a private `Stats` copy whose resource list is the menu resource id repeated by the cost.
//! The game's own `_BuyPartShop` then shows the real cost and deducts the bar's stock.

use std::{
    collections::{HashMap, HashSet},
    ffi::{c_char, CString},
    mem::{offset_of, size_of},
    num::NonZeroU32,
};

use highfleet::general::{EscadraString, Node};
use highfleet::v1_163::{Body, PartStats};
use patchy::{CallBridge, FlagPolicy, KeepReturn, PatchSession, Result, StackAlignment};

use crate::{
    config::{PartMenuConfig, PartMenuResource},
    rng,
};

#[cfg(not(feature = "1_151"))]
mod addresses {
    /// `New` / `operator new`.
    pub const NEW: usize = 0x140334094;
    /// `Node::Node`.
    pub const NODE_CONSTRUCTOR: usize = 0x14027f200;
    /// `CopyString(dest, src, len)`.
    pub const COPY_STRING: usize = 0x1400056e0;
    /// `DefinePart(ALL_PARTS, oid, library, count)`.
    pub const DEFINE_PART: usize = 0x14021b190;
    /// The global `ALL_PARTS` library pointer.
    pub const ALL_PARTS: usize = 0x143a139f0;
    /// `BottomMenu_(hovered element, hovered body)`.
    pub const BOTTOM_MENU: usize = 0x14020d820;
    /// `ClickFunc(context, gui, button, value)`.
    pub const CLICK_FUNC: usize = 0x14012f120;

    /// The `CALL BottomMenu_` in `mainMap`.
    pub const HOVER_CALL_SITE: usize = 0x14014074a;
    /// The `CALL ClickFunc` in `HandleClick`.
    pub const CLICK_CALL_SITE: usize = 0x14012d168;

    /// `EditorLibraryHull`; the palette global used for the rename trick.
    pub const HULL_SLOT: usize = 0x1403bd400;
    pub const ARMOR_SLOT: usize = 0x1403bd418;
    pub const FUEL_SLOT: usize = 0x1403bd430;
    pub const AMMO_SLOT: usize = 0x1403bd448;
    pub const LEG_SLOT: usize = 0x1403bd460;

    /// `PARTS_CONSTRUCTOR_SERIA` (the constructor row's library).
    pub const CONSTRUCTOR_LIBRARY: usize = 0x1403ef4f8;
    /// `CURRENT_LOCATION` (the city object).
    pub const CURRENT_LOCATION: usize = 0x143a11940;
    /// `PLAYER_ESCADRA_`; its `+0x520` node is the player's part inventory.
    pub const PLAYER_ESCADRA: usize = 0x143a13eb0;
    /// `LOAD_CONSTRUCTOR_OR_EDITOR` (non-zero while the editor is open).
    pub const LOAD_CONSTRUCTOR_OR_EDITOR: usize = 0x1403ef36a;

    /// Runtime resource table: `EscadraName` records, stride `0x40`.
    pub const RESOURCE_TABLE: usize = 0x143a13f10;
    /// The global stats pointer vector (`StatListBase`).
    pub const STAT_LIST_BASE: usize = 0x143a33bf0;
    /// `initStats1`: default-constructs and deep-copies a `Stats` record.
    pub const INIT_STATS_COPY: usize = 0x1402815d0;
    /// `GetStats(part_index)`.
    pub const GET_STATS: usize = 0x140281e00;
    /// The game CRT `free`, matching `New`.
    pub const FREE: usize = 0x1403340d0;
}

#[cfg(not(feature = "1_151"))]
use addresses::*;

/// `CURRENT_LOCATION + shop node` (the city object is not modeled in `highfleet-rs`).
const SHOP_NODE_OFFSET: usize = 0x348;
/// `CURRENT_LOCATION + city type`.
const CITY_TYPE_OFFSET: usize = 0x25c;
/// `PLAYER_ESCADRA + part inventory node`.
const PLAYER_INVENTORY_OFFSET: usize = 0x520;

/// Full native `Stats` record size.
///
/// `highfleet-rs` models only the first `0x150` bytes as [`PartStats`]; `initStats1` allocates this
/// larger native record.
const STATS_SIZE: usize = 0x188;
/// `Stats.m_resources` (a three-pointer `std::vector<u32>`) starts here.
///
/// It sits inside [`PartStats::unknown_008ch`], which starts at `0x8c`; the compile-time assert
/// below pins the two offsets together.
const STATS_RESOURCES_OFFSET: usize = 0x98;
/// Class code of a serialized `Body`.
const CODE_BODY: u64 = 15;

/// First and last unused ids in the game's resource table.
const FIRST_CUSTOM_RESOURCE: u32 = 61;
const LAST_CUSTOM_RESOURCE: u32 = 98;

/// Native `Stats.m_resources` header: a three-pointer `std::vector<u32>`.
#[repr(C)]
struct StatsResources {
    begin: *mut u32,
    end: *mut u32,
    capacity: *mut u32,
}

const _: () = {
    assert!(offset_of!(PartStats, unknown_008ch) == 0x8c);
    assert!(offset_of!(PartStats, unknown_008ch) + 0x0c == STATS_RESOURCES_OFFSET);
    assert!(size_of::<StatsResources>() == 0x18);
};

/// Native `StatList` vector header: base, initialized end, reserved end.
#[repr(C)]
struct StatsList {
    base: *mut *mut PartStats,
    end: *mut *mut PartStats,
    capacity: *mut *mut PartStats,
}

const _: () = {
    assert!(size_of::<StatsList>() == 0x18);
};

unsafe fn as_body(pointer: usize) -> &'static mut Body {
    &mut *(pointer as *mut Body)
}

unsafe fn as_node(pointer: usize) -> &'static mut Node {
    &mut *(pointer as *mut Node)
}

unsafe fn stats_resources(stats: *mut PartStats) -> &'static mut StatsResources {
    &mut *((stats as *mut u8).add(STATS_RESOURCES_OFFSET) as *mut StatsResources)
}

unsafe fn stats_list() -> &'static mut StatsList {
    &mut *(STAT_LIST_BASE as *mut StatsList)
}

/// A part clone in a no-gating menu still needs a large count because the money
/// path decrements the clone.
const UNLIMITED_COUNT: i32 = i32::MAX;

type NewFn = unsafe extern "C" fn(usize) -> usize;
type NodeConstructorFn = unsafe extern "C" fn(usize) -> usize;
type CopyStringFn = unsafe extern "C" fn(usize, *const c_char, usize);
type DefinePartFn = unsafe extern "C" fn(usize, *const c_char, usize, i32) -> usize;
type HoverFn = unsafe extern "C" fn(usize, usize) -> u8;
type ClickFn = unsafe extern "C" fn(usize, usize, u32, f32);
type InitStatsCopyFn = unsafe extern "C" fn(usize, usize) -> usize;
type FreeFn = unsafe extern "C" fn(*mut std::ffi::c_void);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resource {
    Vanilla(u32),
    Custom(u32),
    None,
}

struct RuntimePart {
    oid: String,
    cstr: CString,
    /// `None` means "use the part's native resource count" for a vanilla bar,
    /// or 1 for a custom resource.
    cost: Option<NonZeroU32>,
}

struct RuntimeRule {
    probability: f32,
    min: u32,
    max: u32,
    city_types: Vec<u32>,
}

struct RuntimeMenu {
    bar: String,
    bar_cstr: CString,
    parts: Vec<RuntimePart>,
    resource: Resource,
    rules: Vec<RuntimeRule>,
    /// `Some(palette global)` for a menu that adds parts to a vanilla bar.
    vanilla_slot: Option<usize>,
    /// The private library node, built on first use.
    node: usize,
    /// Part clones inside `node`.
    clones: Vec<usize>,
}

#[derive(Clone, Copy)]
struct CloneStats {
    original: i32,
    private: i32,
}

static mut MENUS: Vec<RuntimeMenu> = Vec::new();
static mut CLONE_STATS: Option<HashMap<usize, CloneStats>> = None;
static mut CITY_MENUS: Option<HashMap<usize, Vec<bool>>> = None;

fn menus() -> &'static mut Vec<RuntimeMenu> {
    // SAFETY: every access happens on the game's main thread, from the same hooks that own the state.
    // The vector is written once during init.
    unsafe { &mut *std::ptr::addr_of_mut!(MENUS) }
}

fn clone_stats() -> &'static mut HashMap<usize, CloneStats> {
    // SAFETY: see `menus`.
    unsafe { (*std::ptr::addr_of_mut!(CLONE_STATS)).get_or_insert_with(HashMap::new) }
}

fn city_map() -> &'static mut HashMap<usize, Vec<bool>> {
    // SAFETY: see `menus`.
    unsafe { (*std::ptr::addr_of_mut!(CITY_MENUS)).get_or_insert_with(HashMap::new) }
}

/// Prepares the hooks for the configured part menus.
///
/// # Safety
///
/// Must be called during mod init, after the game's image has been loaded and
/// before the first shop or shipworks screen is opened.
pub(crate) unsafe fn prepare(session: &mut PatchSession, configs: &[PartMenuConfig]) -> Result {
    let runtime = build_runtime(configs);
    if runtime.is_empty() {
        log::warn!("Part menus: no usable menus configured, skipping the hooks");
        return Ok(());
    }

    let custom_bars = runtime
        .iter()
        .filter(|menu| menu.vanilla_slot.is_none())
        .count();
    log::info!(
        "Part menus: {} menu(s) configured ({} new bar(s), {} added to vanilla bars)",
        runtime.len(),
        custom_bars,
        runtime.len() - custom_bars
    );
    for menu in &runtime {
        log::info!(
            "Part menus: \"{}\": {} part(s), resource {}, {} rule(s)",
            menu.bar,
            menu.parts.len(),
            match menu.resource {
                Resource::Vanilla(id) => format!("vanilla id {id}"),
                Resource::Custom(id) => format!("custom id {id}"),
                Resource::None => "none".to_string(),
            },
            menu.rules.len()
        );
    }

    *menus() = runtime;
    rng::seed();

    install(
        session,
        HOVER_CALL_SITE,
        hover as *const (),
        KeepReturn::Rax,
        "part menu hover",
    );
    install(
        session,
        CLICK_CALL_SITE,
        click as *const (),
        KeepReturn::Neither,
        "part menu click guard",
    );

    Ok(())
}

unsafe fn install(
    session: &mut PatchSession,
    address: usize,
    callback: *const (),
    keep_return: KeepReturn,
    name: &str,
) {
    let bridge = CallBridge {
        callback,
        incoming_stack: StackAlignment::Aligned16,
        flags: FlagPolicy::Preserve,
        keep_return,
        before_call: &[],
        after_call: &[],
    };
    match session.patch_call(address, 5, bridge) {
        Ok(_) => log::info!("Part menus: {name} hook at {address:#x}"),
        Err(error) => {
            log::error!("Part menus: could not hook {name} at {address:#x}: {error}");
        }
    }
}

fn build_runtime(configs: &[PartMenuConfig]) -> Vec<RuntimeMenu> {
    let mut menus = Vec::new();
    let mut used_bars = HashSet::new();
    let mut next_custom = FIRST_CUSTOM_RESOURCE;
    // Reserve explicit custom ids first so automatic allocation does not take them.
    let mut used_ids: HashSet<u32> = configs
        .iter()
        .filter_map(|config| match config.resource {
            Some(PartMenuResource::CustomId(id)) => Some(id),
            _ => None,
        })
        .collect();

    for (index, config) in configs.iter().enumerate() {
        let bar_text = config.bar.trim();
        if bar_text.is_empty() {
            log::error!("Part menus: entry {index} has an empty bar id, skipping it");
            continue;
        }

        let vanilla = vanilla_bar(bar_text);
        let bar = match vanilla {
            Some((_, _, canonical)) => canonical.to_string(),
            None => bar_text.to_string(),
        };

        if !used_bars.insert(bar.to_ascii_uppercase()) {
            log::error!(
                "Part menus: \"{}\" is used by more than one menu, skipping the duplicate",
                bar
            );
            continue;
        }

        let mut parts = Vec::new();
        for part in &config.parts {
            let (oid, cost) = part.oid_and_cost();
            let oid = oid.trim();
            if oid.is_empty() {
                continue;
            }
            let cstr = match CString::new(oid) {
                Ok(value) => value,
                Err(_) => {
                    log::error!(
                        "Part menus: \"{}\" contains a NUL byte, skipping the part",
                        oid
                    );
                    continue;
                }
            };
            parts.push(RuntimePart {
                oid: oid.to_string(),
                cstr,
                cost,
            });
        }

        if parts.is_empty() {
            log::warn!(
                "Part menus: \"{}\" has no usable parts, skipping the menu",
                bar
            );
            continue;
        }

        let resource = match resolve_resource(
            config.resource,
            vanilla,
            &mut next_custom,
            &mut used_ids,
            &bar,
        ) {
            Ok(value) => value,
            Err(message) => {
                log::error!("Part menus: \"{bar}\": {message}, skipping the menu");
                continue;
            }
        };

        let mut rules = Vec::new();
        for rule in &config.rules {
            rules.push(RuntimeRule {
                probability: rule.probability,
                min: rule.min_parts,
                max: rule.max_parts,
                city_types: rule.city_types.clone(),
            });
        }
        if rules.is_empty() {
            rules.push(RuntimeRule {
                probability: 1.0,
                min: 1,
                max: 1,
                city_types: Vec::new(),
            });
        }

        let bar_cstr = match CString::new(bar.clone()) {
            Ok(value) => value,
            Err(_) => {
                log::error!("Part menus: \"{bar}\" contains a NUL byte, skipping the menu");
                continue;
            }
        };

        menus.push(RuntimeMenu {
            bar,
            bar_cstr,
            parts,
            resource,
            rules,
            vanilla_slot: vanilla.map(|(_, slot, _)| slot),
            node: 0,
            clones: Vec::new(),
        });
    }

    menus
}

fn resolve_resource(
    choice: Option<PartMenuResource>,
    vanilla: Option<(u32, usize, &'static str)>,
    next_custom: &mut u32,
    used_ids: &mut HashSet<u32>,
    bar: &str,
) -> std::result::Result<Resource, String> {
    match choice {
        None => match vanilla {
            Some((id, _, _)) => Ok(Resource::Vanilla(id)),
            None => allocate_custom(next_custom, used_ids).map(Resource::Custom),
        },
        Some(PartMenuResource::None) => Ok(Resource::None),
        Some(PartMenuResource::Custom) => {
            if vanilla.is_some() {
                Err("a vanilla bar cannot use a custom resource".to_string())
            } else {
                allocate_custom(next_custom, used_ids).map(Resource::Custom)
            }
        }
        Some(PartMenuResource::Vanilla(id)) => vanilla_resource(vanilla, id, bar),
        Some(PartMenuResource::CustomId(id)) => {
            if vanilla.is_some() {
                return Err("a vanilla bar cannot use a custom resource".to_string());
            }
            Ok(Resource::Custom(id))
        }
    }
}

fn vanilla_resource(
    vanilla: Option<(u32, usize, &'static str)>,
    id: u32,
    bar: &str,
) -> std::result::Result<Resource, String> {
    match vanilla {
        Some((vanilla_id, _, _)) if vanilla_id == id => Ok(Resource::Vanilla(id)),
        Some(_) => Err(format!("bar \"{bar}\" does not match resource id {id}")),
        None => Err(format!(
            "resource id {id} needs a vanilla bar (ITEM_HULL, ITEM_ARMOR, ITEM_FUEL, ITEM_AMMO or ITEM_LEG)"
        )),
    }
}

fn allocate_custom(
    next_custom: &mut u32,
    used_ids: &mut HashSet<u32>,
) -> std::result::Result<u32, String> {
    let mut id = (*next_custom).max(FIRST_CUSTOM_RESOURCE);
    while id <= LAST_CUSTOM_RESOURCE {
        if used_ids.insert(id) {
            *next_custom = id + 1;
            return Ok(id);
        }
        id += 1;
    }
    Err(format!(
        "no free custom resource id left in {FIRST_CUSTOM_RESOURCE}..={LAST_CUSTOM_RESOURCE}"
    ))
}

fn vanilla_bar(bar: &str) -> Option<(u32, usize, &'static str)> {
    match bar.to_ascii_uppercase().as_str() {
        "ITEM_HULL" => Some((10, HULL_SLOT, "ITEM_HULL")),
        "ITEM_ARMOR" => Some((20, ARMOR_SLOT, "ITEM_ARMOR")),
        "ITEM_FUEL" => Some((30, FUEL_SLOT, "ITEM_FUEL")),
        "ITEM_AMMO" => Some((40, AMMO_SLOT, "ITEM_AMMO")),
        "ITEM_LEG" => Some((50, LEG_SLOT, "ITEM_LEG")),
        _ => None,
    }
}

/// Called from the shop-generation hook after the city shop node exists.
///
/// # Safety
///
/// Must run on the game's main thread while `shop_node` is a live city shop
/// node and `ALL_PARTS` has been loaded.
pub(crate) unsafe fn inject_menus(shop_node: usize, city: usize, city_type: Option<u32>) {
    if shop_node == 0 {
        return;
    }

    let menu_count = menus().len();
    let mut city_bits = vec![false; menu_count];

    for (index, city_enabled) in city_bits.iter_mut().enumerate() {
        let (vanilla, bar, bar_cstr, resource) = {
            let menu = &menus()[index];
            (
                menu.vanilla_slot.is_some(),
                menu.bar.clone(),
                menu.bar_cstr.clone(),
                menu.resource,
            )
        };

        let rule = {
            let menu = &menus()[index];
            pick_rule(&menu.rules, city_type).map(|rule| (rule.probability, rule.min, rule.max))
        };
        let Some((probability, min, max)) = rule else {
            continue;
        };

        let enabled = rng::random_f32() < probability;
        *city_enabled = enabled;
        if !enabled || vanilla {
            continue;
        }

        if let Resource::Custom(id) = resource {
            write_resource_name(id, &bar);
        }

        if !ensure_built(index) {
            continue;
        }

        let count = rng::random_range(min, max) as i32;
        let clone = define_part(&bar_cstr, shop_node, count);
        if clone == 0 {
            log::error!(
                "Part menus: could not create bar \"{bar}\" (unknown part id or missing stats in OL.seria)"
            );
            continue;
        }

        let body = as_body(clone);
        body.count = count;
        body.is_loot = 1;
        body.is_blueprint = 0;

        let price = bar_price(clone);
        if price > 0 {
            log::warn!(
                "Part menus: bar \"{bar}\" has price {price}; every resource unit will also cost that much money (use a loot/item oid with price 0 to avoid this)"
            );
        }
        log::debug!("Part menus: \"{bar}\" spawned with {count} stock");
    }

    if city != 0 {
        city_map().insert(city, city_bits);
    }
}

unsafe fn ensure_built(index: usize) -> bool {
    if menus()[index].node != 0 {
        return true;
    }

    let node = create_node("PART_MENU");
    if node == 0 {
        log::error!("Part menus: could not allocate a library node");
        return false;
    }

    let (bar, resource, parts_len) = {
        let menu = &menus()[index];
        (menu.bar.clone(), menu.resource, menu.parts.len())
    };

    // Vanilla resource bars are loot prototypes.
    // Make the configured bar prototype loot too, so a bought bar is a resource item rather than a placeable hull body.
    mark_prototype_loot(&bar);

    if let Resource::Custom(id) = resource {
        if !write_resource_name(id, &bar) {
            return false;
        }
    }

    let unlimited = resource == Resource::None;
    let count = if unlimited { UNLIMITED_COUNT } else { 1 };
    let mut added = 0;
    for part_index in 0..parts_len {
        let (cstr, oid, configured_cost) = {
            let menu = &menus()[index];
            let part = &menu.parts[part_index];
            (part.cstr.clone(), part.oid.clone(), part.cost)
        };
        let clone = define_part(&cstr, node, count);
        if clone == 0 {
            log::warn!("Part menus: \"{bar}\": part \"{oid}\" does not exist, skipping it");
            continue;
        }
        let clone_body = as_body(clone);
        clone_body.count = count;
        clone_body.is_blueprint = 0;
        let cost = configured_cost.map(NonZeroU32::get).unwrap_or(1);
        menus()[index].parts[part_index].cost = NonZeroU32::new(cost);
        let resource_id = match resource {
            Resource::None => None,
            Resource::Vanilla(id) | Resource::Custom(id) => Some(id),
        };
        register_clone(clone, resource_id, cost);
        added += 1;
        menus()[index].clones.push(clone);
    }

    menus()[index].node = node;
    log::info!(
        "Part menus: built \"{bar}\" with {added}/{parts_len} part(s), resource {}",
        match resource {
            Resource::Vanilla(id) => format!("vanilla id {id}"),
            Resource::Custom(id) => format!("custom id {id}"),
            Resource::None => "none".to_string(),
        }
    );
    true
}

unsafe fn ensure_augmented(index: usize, editor: bool) {
    let menu_count = menus().len();
    let (slot, parts_len, resource) = {
        let menu = &menus()[index];
        let Some(slot) = menu.vanilla_slot else {
            return;
        };
        (slot, menu.parts.len(), menu.resource)
    };

    if !editor {
        let city = *(CURRENT_LOCATION as *const usize);
        if city == 0 {
            return;
        }
        let enabled = city_map()
            .get(&city)
            .and_then(|bits| bits.get(index).copied());
        let enabled = match enabled {
            Some(value) => value,
            None => {
                let city_type = current_city_type(city);
                let value = match pick_rule(&menus()[index].rules, city_type) {
                    Some(rule) => rng::random_f32() < rule.probability,
                    None => false,
                };
                city_map()
                    .entry(city)
                    .or_insert_with(|| vec![false; menu_count])[index] = value;
                value
            }
        };
        if !enabled {
            return;
        }
    }

    let node = *(slot as *const usize);
    if node == 0 {
        return;
    }

    let no_gating = resource == Resource::None;
    let unlimited = editor || no_gating;
    let count = if unlimited { UNLIMITED_COUNT } else { 1 };
    let bar = menus()[index].bar.clone();

    for part_index in 0..parts_len {
        let (cstr, oid, configured_cost) = {
            let menu = &menus()[index];
            let part = &menu.parts[part_index];
            (part.cstr.clone(), part.oid.clone(), part.cost)
        };

        if let Some(existing) = find_child(node, &oid) {
            apply_clone_state(existing, editor, no_gating);
            continue;
        }

        let clone = define_part(&cstr, node, count);
        if clone == 0 {
            log::warn!("Part menus: \"{bar}\": part \"{oid}\" does not exist, skipping it");
            continue;
        }
        let native_cost = resource_count(clone);
        let cost = configured_cost
            .map(NonZeroU32::get)
            .unwrap_or(match resource {
                Resource::Vanilla(_) => native_cost.max(1),
                _ => 1,
            });
        let resource_id = match resource {
            Resource::None => None,
            Resource::Vanilla(id) | Resource::Custom(id) => Some(id),
        };
        menus()[index].parts[part_index].cost = NonZeroU32::new(cost);
        register_clone(clone, resource_id, cost);
        apply_clone_state(clone, editor, no_gating);
        log::debug!(
            "Part menus: added part to the \"{bar}\" menu (cost {cost} unit(s), resource {})",
            match resource {
                Resource::Vanilla(id) => format!("id {id}"),
                Resource::Custom(id) => format!("id {id}"),
                Resource::None => "none".to_string(),
            }
        );
    }
}

/// Creates or remembers the private stats record for one menu clone.
///
/// Gated parts get a private `Stats` copy whose resource list is the menu's
/// resource id repeated `cost` times.
/// That is what makes the shop show a real resource cost and lets the game's own
/// `_BuyPartShop` deduct the bar stock.
unsafe fn register_clone(clone: usize, resource_id: Option<u32>, cost: u32) {
    let original = as_body(clone).part_index;
    let private = match resource_id {
        Some(id) => create_private_stats(original, id, cost),
        None => original,
    };
    clone_stats().insert(clone, CloneStats { original, private });
}

unsafe fn create_private_stats(original: i32, resource_id: u32, cost: u32) -> i32 {
    let source = get_stats_by_index(original);
    if source.is_null() {
        log::error!(
            "Part menus: cannot create private stats for part stats index {original} (not loaded); its purchase will fail"
        );
        return original;
    }

    let new: NewFn = std::mem::transmute(NEW);
    let memory = new(STATS_SIZE);
    if memory == 0 {
        log::error!("Part menus: out of memory creating private part stats");
        return original;
    }

    let init: InitStatsCopyFn = std::mem::transmute(INIT_STATS_COPY);
    let stats = init(memory, source as usize) as *mut PartStats;
    if stats.is_null() {
        log::error!("Part menus: the stats copy constructor returned null");
        return original;
    }

    set_stats_resources(stats, resource_id, cost);
    match append_stat(stats) {
        Some(index) => {
            log::debug!(
                "Part menus: private stats index {index} (resource id {resource_id} x{cost})"
            );
            index
        }
        None => {
            log::error!("Part menus: could not append private part stats to the game list");
            original
        }
    }
}

unsafe fn set_stats_resources(stats: *mut PartStats, resource_id: u32, cost: u32) {
    let resources = stats_resources(stats);
    let begin = resources.begin;
    let capacity = resources.capacity;
    let count = cost as usize;
    let capacity_count = if begin.is_null() {
        0
    } else {
        (capacity as usize - begin as usize) / size_of::<u32>()
    };

    if !begin.is_null() && capacity_count >= count {
        for index in 0..count {
            *begin.add(index) = resource_id;
        }
        resources.end = begin.add(count);
        return;
    }

    let new: NewFn = std::mem::transmute(NEW);
    let buffer = new(count * size_of::<u32>()) as *mut u32;
    if buffer.is_null() {
        return;
    }
    for index in 0..count {
        *buffer.add(index) = resource_id;
    }
    if !begin.is_null() {
        let free: FreeFn = std::mem::transmute(FREE);
        free(begin as *mut std::ffi::c_void);
    }

    resources.begin = buffer;
    resources.end = buffer.add(count);
    resources.capacity = resources.end;
}

/// Appends a stats record to the game's `StatList` vector and returns its index,
/// or `None` when the vector could not be grown.
unsafe fn append_stat(record: *mut PartStats) -> Option<i32> {
    let list = stats_list();
    if list.base.is_null() {
        return None;
    }

    let count = list.end.offset_from(list.base) as usize;
    if list.end < list.capacity {
        *list.end = record;
        list.end = list.end.add(1);
        return Some(count as i32);
    }

    let capacity_count = list.capacity.offset_from(list.base) as usize;
    let new_capacity = (capacity_count * 2).max(count + 1);
    let new: NewFn = std::mem::transmute(NEW);
    let new_base = new(new_capacity * size_of::<*mut PartStats>()) as *mut *mut PartStats;
    if new_base.is_null() {
        return None;
    }

    if count != 0 {
        std::ptr::copy_nonoverlapping(list.base, new_base, count);
    }
    let free: FreeFn = std::mem::transmute(FREE);
    free(list.base as *mut std::ffi::c_void);

    *new_base.add(count) = record;
    list.base = new_base;
    list.end = new_base.add(count + 1);
    list.capacity = new_base.add(new_capacity);
    Some(count as i32)
}

/// Sets a clone's per-mode state: free and unlimited in the editor (or a
/// no-gating menu), and the private resource stats only while a shop offers it.
unsafe fn apply_clone_state(clone: usize, editor: bool, no_gating: bool) {
    let free_mode = editor || no_gating;
    let body = as_body(clone);
    body.count = if free_mode { UNLIMITED_COUNT } else { 1 };
    body.is_blueprint = u8::from(!free_mode);
    if let Some(stats) = clone_stats().get(&clone).copied() {
        body.part_index = if free_mode {
            stats.original
        } else {
            stats.private
        };
    }
}

unsafe extern "C" fn hover(hovered: usize, body: usize) -> u8 {
    let original: HoverFn = std::mem::transmute(BOTTOM_MENU);
    let Some(oid) = body_oid(body) else {
        return original(hovered, body);
    };

    let editor = *(LOAD_CONSTRUCTOR_OR_EDITOR as *const u8) != 0;

    if let Some(index) = find_menu(&oid, false) {
        if !bar_in_current_row(body, editor) {
            return original(hovered, body);
        }
        if !ensure_built(index) {
            return original(hovered, body);
        }

        let (node, no_gating, clone_count) = {
            let menu = &menus()[index];
            (
                menu.node,
                menu.resource == Resource::None,
                menu.clones.len(),
            )
        };
        for clone_index in 0..clone_count {
            let clone = menus()[index].clones[clone_index];
            apply_clone_state(clone, editor, no_gating);
        }

        let Some(saved) = rename_to_hull(body) else {
            return original(hovered, body);
        };

        let slot = HULL_SLOT as *mut usize;
        let saved_node = *slot;
        *slot = node;
        let result = original(hovered, body);
        *slot = saved_node;
        restore_oid(body, &saved);
        return result;
    }

    if let Some(index) = find_menu(&oid, true) {
        if bar_in_current_row(body, editor) {
            ensure_augmented(index, editor);
        }
    }

    original(hovered, body)
}

/// True when `body` is a direct child of the row the player is looking at.
unsafe fn bar_in_current_row(body: usize, editor: bool) -> bool {
    if editor {
        return contains_child(*(CONSTRUCTOR_LIBRARY as *const usize), body);
    }
    let city = *(CURRENT_LOCATION as *const usize);
    if city != 0 && contains_child(*((city + SHOP_NODE_OFFSET) as *const usize), body) {
        return true;
    }
    // A bar bought into the player's inventory is still a menu there.
    contains_child(player_inventory_node(), body)
}

unsafe fn player_inventory_node() -> usize {
    let player = *(PLAYER_ESCADRA as *const usize);
    if player == 0 {
        return 0;
    }
    *((player + PLAYER_INVENTORY_OFFSET) as *const usize)
}

unsafe fn contains_child(node_pointer: usize, body: usize) -> bool {
    if node_pointer == 0 || body == 0 {
        return false;
    }

    let sentinel = as_node(node_pointer).children.head;
    if sentinel.is_null() {
        return false;
    }

    let mut current = (*sentinel).next;
    while current != sentinel {
        if (*current).value as usize == body {
            return true;
        }
        current = (*current).next;
    }
    false
}

/// `HandleClick`'s `CALL ClickFunc` replacement.
unsafe extern "C" fn click(context: usize, gui: usize, button: u32, value: f32) {
    let original: ClickFn = std::mem::transmute(CLICK_FUNC);
    // Only the editor attaches a palette button to the ship.
    // The city shop's list buys its rows, so renaming there would make a bought bar change its oid.
    let editor = *(LOAD_CONSTRUCTOR_OR_EDITOR as *const u8) != 0;
    if button != 1 || !editor {
        original(context, gui, button, value);
        return;
    }

    let saved = begin_click_rename();
    original(context, gui, button, value);
    end_click_rename(&saved);
}

unsafe fn begin_click_rename() -> Vec<(usize, String)> {
    let mut saved = Vec::new();
    let editor = *(LOAD_CONSTRUCTOR_OR_EDITOR as *const u8) != 0;
    let node = if editor {
        *(CONSTRUCTOR_LIBRARY as *const usize)
    } else {
        let city = *(CURRENT_LOCATION as *const usize);
        if city == 0 {
            0
        } else {
            *((city + SHOP_NODE_OFFSET) as *const usize)
        }
    };
    if node == 0 {
        return saved;
    }

    let sentinel = as_node(node).children.head;
    if sentinel.is_null() {
        return saved;
    }

    let mut current = (*sentinel).next;
    while current != sentinel {
        let payload = (*current).value;
        if !payload.is_null() {
            if let Some(oid) = body_oid(payload as usize) {
                if is_custom_bar(&oid) {
                    if let Some(save) = rename_to_hull(payload as usize) {
                        saved.push((payload as usize, save));
                    }
                }
            }
        }
        current = (*current).next;
    }

    saved
}

unsafe fn end_click_rename(saved: &[(usize, String)]) {
    for (body, save) in saved {
        restore_oid(*body, save);
    }
}

type GetStatsFn = unsafe extern "C" fn(i32) -> *mut PartStats;

unsafe fn get_stats_by_index(index: i32) -> *mut PartStats {
    let get: GetStatsFn = std::mem::transmute(GET_STATS);
    get(index)
}

unsafe fn get_stats(part: usize) -> *mut PartStats {
    get_stats_by_index(as_body(part).part_index)
}

/// Returns how many resource entries the part's shared stats currently have.
unsafe fn resource_count(part: usize) -> u32 {
    let stats = get_stats(part);
    if stats.is_null() {
        return 0;
    }
    let resources = stats_resources(stats);
    if resources.end <= resources.begin {
        return 0;
    }
    ((resources.end as usize - resources.begin as usize) / size_of::<u32>()) as u32
}

/// Returns the part's money price (`Stats.m_price`).
unsafe fn bar_price(part: usize) -> i32 {
    let stats = get_stats(part);
    if stats.is_null() {
        return 0;
    }
    (*stats).price
}

fn find_menu(oid: &str, augmentation: bool) -> Option<usize> {
    menus()
        .iter()
        .position(|menu| menu.vanilla_slot.is_some() == augmentation && menu.bar == oid)
}

fn is_custom_bar(oid: &str) -> bool {
    find_menu(oid, false).is_some()
}

fn pick_rule(rules: &[RuntimeRule], city_type: Option<u32>) -> Option<&RuntimeRule> {
    rules.iter().find(|rule| {
        rule.city_types.is_empty() || city_type.is_some_and(|city| rule.city_types.contains(&city))
    })
}

unsafe fn current_city_type(city: usize) -> Option<u32> {
    if city == 0 {
        return None;
    }
    let value = *((city + CITY_TYPE_OFFSET) as *const i32);
    if (1..=7).contains(&value) {
        Some(value as u32)
    } else {
        None
    }
}

unsafe fn create_node(name: &str) -> usize {
    let new: NewFn = std::mem::transmute(NEW);
    let raw = new(size_of::<Node>());
    if raw == 0 {
        return 0;
    }

    let constructor: NodeConstructorFn = std::mem::transmute(NODE_CONSTRUCTOR);
    let node = constructor(raw);
    if node == 0 {
        return 0;
    }

    let copy: CopyStringFn = std::mem::transmute(COPY_STRING);
    let name_field = &mut as_node(node).name;
    copy(
        name_field as *mut EscadraString as usize,
        name.as_ptr() as *const c_char,
        name.len(),
    );
    node
}

unsafe fn define_part(oid: &CString, node: usize, count: i32) -> usize {
    let all_parts = *(ALL_PARTS as *const usize);
    if all_parts == 0 || node == 0 {
        return 0;
    }

    let define: DefinePartFn = std::mem::transmute(DEFINE_PART);
    define(all_parts, oid.as_ptr(), node, count) as usize
}

unsafe fn body_oid(part: usize) -> Option<String> {
    if part == 0 {
        return None;
    }

    let body = as_body(part);
    if body.base.base.base.code != CODE_BODY {
        return None;
    }

    let oid = body.oid.get_string();
    if oid.is_empty() {
        None
    } else {
        Some(oid.to_owned())
    }
}

unsafe fn rename_to_hull(part: usize) -> Option<String> {
    if part == 0 {
        return None;
    }

    let body = as_body(part);
    if body.base.base.base.code != CODE_BODY {
        return None;
    }

    let saved = body.oid.get_string().to_owned();
    body.oid.set_string(&"ITEM_HULL".to_owned());
    Some(saved)
}

unsafe fn restore_oid(part: usize, saved: &str) {
    as_body(part).oid.set_string(&saved.to_owned());
}

unsafe fn record_name(record: usize) -> Option<String> {
    let name = &*(record as *const EscadraString);
    let text = name.get_string();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

unsafe fn write_resource_name(id: u32, name: &str) -> bool {
    let record = RESOURCE_TABLE + id as usize * 0x40;
    if let Some(existing) = record_name(record) {
        if existing == name {
            return true;
        }

        log::error!(
            "Part menus: resource id {id} is already named \"{existing}\", cannot use \"{name}\""
        );
        return false;
    }

    let copy: CopyStringFn = std::mem::transmute(COPY_STRING);
    copy(record, name.as_ptr() as *const c_char, name.len());
    true
}

/// Finds the direct child body with `oid` under `node`.
unsafe fn find_child(node_pointer: usize, oid: &str) -> Option<usize> {
    if node_pointer == 0 {
        return None;
    }

    let node = as_node(node_pointer);
    let sentinel = node.children.head;
    if sentinel.is_null() {
        return None;
    }

    let mut current = (*sentinel).next;
    while current != sentinel {
        let child = (*current).value;
        if !child.is_null() {
            let body = child as *mut Body;
            if (*body).base.base.base.code == CODE_BODY && (*body).oid.get_string() == oid {
                return Some(child as usize);
            }
        }
        current = (*current).next;
    }
    None
}

/// Marks the bar's prototype in `ALL_PARTS` as loot.
///
/// Vanilla resource bars (`ITEM_HULL`, ...) have `is_loot=true` in `parts.seria`.
/// A custom bar oid usually does not, and the game then treats a bought bar as a placeable body, which floats when dragged out of the inventory.
/// The bar clone gets the flag too, but the placement path looks the prototype up by oid.
unsafe fn mark_prototype_loot(oid: &str) {
    let all_parts = *(ALL_PARTS as *const usize);
    if all_parts == 0 {
        return;
    }

    match find_child(all_parts, oid) {
        Some(prototype) => {
            let body = as_body(prototype);
            if body.is_loot == 0 {
                body.is_loot = 1;
                log::debug!("Part menus: marked prototype \"{oid}\" as loot");
            }
        }
        None => log::warn!("Part menus: bar prototype \"{oid}\" was not found in ALL_PARTS"),
    }
}
