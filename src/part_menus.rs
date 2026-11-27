//! Custom part menus: shop bars that open a list of configured parts.
//!
//! Vanilla city shops only sell a handful of resource bars (`ITEM_HULL`,
//! `ITEM_ARMOR`, ...), and every part without a resource is pushed into the shop
//! as its own row. This module adds menus: a bar with its own stock, whose
//! flyout is built from a private library node.
//!
//! The pieces:
//!
//! * `inject_menus` is called from the shop-generation hook in [`crate::parts`]
//!   and pushes one bar per matching rule into the city's shop node.
//! * `hover` wraps the single `BottomMenu_` call in `mainMap`. For one of our bar
//!   oids it renames the hovered body to `ITEM_HULL` and points the hull palette
//!   global at our menu node for the duration of the call, so the game builds
//!   its own flyout from our parts. The original return value is preserved.
//! * `click` wraps the `ClickFunc` call. Vanilla bars are protected by the
//!   game's own 14-name check; our bars are renamed to `ITEM_HULL` around that
//!   call so clicking one never picks it up as a part.
//! * Every gated menu part gets a private `Stats` copy whose resource list is
//!   the menu's resource id repeated `cost` times. The shop flyout therefore
//!   shows the real cost, and the game's own `_BuyPartShop` finds the bar by
//!   resource id and deducts the stock. The shared part data is never edited;
//!   the clone only points at the private record while a shop is showing it.
//!
//! The resource ids of new menus are written into the game's runtime resource
//! table (base `0x143a13f10`, stride `0x40`), in the unused range `61..=98`.

use std::{
    collections::{HashMap, HashSet},
    ffi::{c_char, CString},
    slice,
};

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
    /// `_BuyPartShop(part, count, target library)`.
    pub const BUY_PART_SHOP: usize = 0x140221400;

    /// The `CALL BottomMenu_` in `mainMap`.
    pub const HOVER_CALL_SITE: usize = 0x14014074a;
    /// The `CALL ClickFunc` in `HandleClick`.
    pub const CLICK_CALL_SITE: usize = 0x14012d168;
    /// The `CALL _BuyPartShop` inside `ObjectList_BuyPart`.
    pub const BUY_CALL_SITE: usize = 0x14016301f;

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
    pub const STAT_LIST_END: usize = 0x143a33bf8;
    pub const STAT_LIST_CAPACITY: usize = 0x143a33c00;
    /// `initStats1`: default-constructs and deep-copies a `Stats` record.
    pub const INIT_STATS_COPY: usize = 0x1402815d0;
    /// The game CRT `free`, matching `New`.
    pub const FREE: usize = 0x1403340d0;
}

#[cfg(not(feature = "1_151"))]
use addresses::*;

/// `CURRENT_LOCATION + shop node`.
const SHOP_NODE_OFFSET: usize = 0x348;
const CITY_TYPE_OFFSET: usize = 0x25c;
/// `PLAYER_ESCADRA + part inventory node`.
const PLAYER_INVENTORY_OFFSET: usize = 0x520;

/// `Body` fields.
const BODY_OID: usize = 0x190;
const BODY_OID_LENGTH: usize = 0x1a0;
const BODY_OID_CAPACITY: usize = 0x1a8;
const BODY_COUNT: usize = 0x7a8;
const BODY_IS_LOOT: usize = 0x7ac;
const BODY_IS_BLUEPRINT: usize = 0x7ad;
const BODY_STATS_INDEX: usize = 0x188;

/// `Stats` resource vector (`std::vector<u32>`).
const STATS_RESOURCES_BEGIN: usize = 0x98;
const STATS_RESOURCES_END: usize = 0xa0;
const STATS_RESOURCES_CAPACITY: usize = 0xa8;
/// Size of a `Stats` record.
const STATS_SIZE: usize = 0x188;

/// `Node` fields.
const NODE_CHILDREN: usize = 0x100;
const NODE_CHILD_PAYLOAD: usize = 0x10;
const NODE_NAME: usize = 0xc8;
const NODE_SIZE: usize = 0x128;

/// First and last unused ids in the game's resource table.
const FIRST_CUSTOM_RESOURCE: u32 = 61;
const LAST_CUSTOM_RESOURCE: u32 = 98;

/// Maximum resource cost accepted for one part.
const MAX_PART_COST: u32 = 64;

/// A part clone in a no-gating menu still needs a large count because the money
/// path decrements the clone.
const UNLIMITED_COUNT: u32 = 999_999_999;

/// Maximum number of bar bodies renamed around one click.
const MAX_CLICK_RENAMES: usize = 16;

type NewFn = unsafe extern "C" fn(usize) -> usize;
type NodeConstructorFn = unsafe extern "C" fn(usize) -> usize;
type CopyStringFn = unsafe extern "C" fn(usize, *const c_char, usize);
type DefinePartFn = unsafe extern "C" fn(usize, *const c_char, usize, i32) -> usize;
type HoverFn = unsafe extern "C" fn(usize, usize) -> u8;
type ClickFn = unsafe extern "C" fn(usize, usize, u32, f32);
type BuyFn = unsafe extern "C" fn(usize, i32, usize) -> usize;
type InitStatsCopyFn = unsafe extern "C" fn(usize, usize) -> usize;
type FreeFn = unsafe extern "C" fn(*mut std::ffi::c_void);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resource {
    Vanilla(u32),
    Custom(u32),
    None,
}

struct RuntimePart {
    oid: Vec<u8>,
    cstr: CString,
    /// `None` means "use the part's native resource count" for a vanilla bar,
    /// or 1 for a custom resource.
    cost: Option<u32>,
}

struct RuntimeRule {
    probability: f32,
    min: u32,
    max: u32,
    city_types: Vec<u32>,
}

struct RuntimeMenu {
    bar: Vec<u8>,
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

struct SavedOid {
    text: usize,
    bytes: [u8; 10],
    length: u64,
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
    // SAFETY: every access happens on the game's main thread, from the same
    // hooks that own the state. The vector is written once during init.
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
            display(&menu.bar),
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
    install(
        session,
        BUY_CALL_SITE,
        buy as *const (),
        KeepReturn::Rax,
        "part menu purchase",
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
    let mut used_ids = HashSet::new();
    let mut used_bars = HashSet::new();
    let mut next_custom = FIRST_CUSTOM_RESOURCE;

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
                display(bar.as_bytes())
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
                oid: oid.as_bytes().to_vec(),
                cstr,
                cost: cost.map(|value| value.clamp(1, MAX_PART_COST)),
            });
        }

        if parts.is_empty() {
            log::warn!(
                "Part menus: \"{}\" has no usable parts, skipping the menu",
                display(bar.as_bytes())
            );
            continue;
        }

        let resource = match resolve_resource(
            config.resource.as_ref(),
            vanilla,
            &mut next_custom,
            &mut used_ids,
            &bar,
        ) {
            Ok(value) => value,
            Err(message) => {
                log::error!(
                    "Part menus: \"{}\": {message}, skipping the menu",
                    display(bar.as_bytes())
                );
                continue;
            }
        };

        let mut rules = Vec::new();
        for rule in &config.rules {
            let min = rule.min_parts.max(1);
            let max = rule.max_parts.max(min);
            rules.push(RuntimeRule {
                probability: rule.probability.clamp(0.0, 1.0),
                min,
                max,
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
                log::error!(
                    "Part menus: \"{}\" contains a NUL byte, skipping the menu",
                    display(bar.as_bytes())
                );
                continue;
            }
        };

        menus.push(RuntimeMenu {
            bar: bar.as_bytes().to_vec(),
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
    choice: Option<&PartMenuResource>,
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
        Some(PartMenuResource::Name(name)) => {
            let name = name.trim().to_ascii_lowercase();
            match name.as_str() {
                "none" => Ok(Resource::None),
                "custom" => {
                    if vanilla.is_some() {
                        Err("a vanilla bar cannot use a custom resource".to_string())
                    } else {
                        allocate_custom(next_custom, used_ids).map(Resource::Custom)
                    }
                }
                "hull" => vanilla_resource(vanilla, 10, bar),
                "armor" => vanilla_resource(vanilla, 20, bar),
                "fuel" => vanilla_resource(vanilla, 30, bar),
                "ammo" => vanilla_resource(vanilla, 40, bar),
                "leg" => vanilla_resource(vanilla, 50, bar),
                other => Err(format!(
                    "unknown resource \"{other}\" (use a vanilla id, \"custom\" or \"none\")"
                )),
            }
        }
        Some(PartMenuResource::Id(id)) => {
            if let Some((vanilla_id, _, _)) = vanilla {
                return if *id == vanilla_id {
                    Ok(Resource::Vanilla(*id))
                } else {
                    Err(format!(
                        "bar \"{bar}\" uses vanilla resource {vanilla_id}, not {id}"
                    ))
                };
            }
            if !(FIRST_CUSTOM_RESOURCE..=LAST_CUSTOM_RESOURCE).contains(id) {
                return Err(format!(
                    "custom resource id {id} is outside {FIRST_CUSTOM_RESOURCE}..={LAST_CUSTOM_RESOURCE}"
                ));
            }
            if !used_ids.insert(*id) {
                return Err(format!("custom resource id {id} is already used"));
            }
            Ok(Resource::Custom(*id))
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

    for index in 0..menu_count {
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
        city_bits[index] = enabled;
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
                "Part menus: could not create bar \"{}\" (unknown part id or missing stats in OL.seria)",
                display(&bar)
            );
            continue;
        }
        set_i32(clone, BODY_COUNT, count);
        set_u8(clone, BODY_IS_LOOT, 1);
        set_u8(clone, BODY_IS_BLUEPRINT, 0);
        let price = bar_price(clone);
        if price > 0 {
            log::warn!(
                "Part menus: bar \"{}\" has price {price}; every resource unit will also cost that much money (use a loot/item oid with price 0 to avoid this)",
                display(&bar)
            );
        }
        log::debug!(
            "Part menus: \"{}\" spawned with {} stock",
            display(&bar),
            count
        );
    }

    if city != 0 {
        city_map().insert(city, city_bits);
    }
}

unsafe fn ensure_built(index: usize) -> bool {
    if menus()[index].node != 0 {
        return true;
    }

    let node = create_node(b"PART_MENU");
    if node == 0 {
        log::error!("Part menus: could not allocate a library node");
        return false;
    }

    let (bar, resource, parts_len) = {
        let menu = &menus()[index];
        (menu.bar.clone(), menu.resource, menu.parts.len())
    };

    // Vanilla resource bars are loot prototypes. Make the configured bar
    // prototype loot too, so a bar bought into the inventory is a resource item
    // rather than a placeable hull body (which floats when dropped on a ship).
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
        let clone = define_part(&cstr, node, count as i32);
        if clone == 0 {
            log::warn!(
                "Part menus: \"{}\": part \"{}\" does not exist, skipping it",
                display(&bar),
                display(&oid)
            );
            continue;
        }
        set_i32(clone, BODY_COUNT, count as i32);
        set_u8(clone, BODY_IS_BLUEPRINT, 0);
        let cost = configured_cost.unwrap_or(1).clamp(1, MAX_PART_COST);
        menus()[index].parts[part_index].cost = Some(cost);
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
        "Part menus: built \"{}\" with {added}/{parts_len} part(s), resource {}",
        display(&bar),
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

        let clone = define_part(&cstr, node, count as i32);
        if clone == 0 {
            log::warn!(
                "Part menus: \"{}\": part \"{}\" does not exist, skipping it",
                display(&bar),
                display(&oid)
            );
            continue;
        }
        let native_cost = resource_count(clone);
        let cost = configured_cost
            .unwrap_or(match resource {
                Resource::Vanilla(_) => native_cost.max(1),
                _ => 1,
            })
            .clamp(1, MAX_PART_COST);
        let resource_id = match resource {
            Resource::None => None,
            Resource::Vanilla(id) | Resource::Custom(id) => Some(id),
        };
        menus()[index].parts[part_index].cost = Some(cost);
        register_clone(clone, resource_id, cost);
        apply_clone_state(clone, editor, no_gating);
        log::debug!(
            "Part menus: added part to the \"{}\" menu (cost {} unit(s), resource {})",
            display(&bar),
            cost,
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
/// resource id repeated `cost` times. That is what makes the shop show a real
/// resource cost and lets the game's own `_BuyPartShop` deduct the bar stock.
unsafe fn register_clone(clone: usize, resource_id: Option<u32>, cost: u32) {
    let original = *((clone + BODY_STATS_INDEX) as *const i32);
    let private = match resource_id {
        Some(id) => create_private_stats(original, id, cost),
        None => original,
    };
    clone_stats().insert(clone, CloneStats { original, private });
}

unsafe fn create_private_stats(original: i32, resource_id: u32, cost: u32) -> i32 {
    let source = get_stats_by_index(original);
    if source == 0 {
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
    let stats = init(memory, source);
    if stats == 0 {
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

unsafe fn set_stats_resources(stats: usize, resource_id: u32, cost: u32) {
    let base = stats as *mut u8;
    let begin_pointer = base.add(STATS_RESOURCES_BEGIN) as *mut usize;
    let end_pointer = base.add(STATS_RESOURCES_END) as *mut usize;
    let capacity_pointer = base.add(STATS_RESOURCES_CAPACITY) as *mut usize;

    let begin = *begin_pointer as *mut u32;
    let capacity = *capacity_pointer as usize;
    let count = cost as usize;
    let capacity_count = if begin.is_null() {
        0
    } else {
        (capacity - begin as usize) / std::mem::size_of::<u32>()
    };

    if !begin.is_null() && capacity_count >= count {
        for index in 0..count {
            *begin.add(index) = resource_id;
        }
        *end_pointer = begin as usize + count * std::mem::size_of::<u32>();
        return;
    }

    let new: NewFn = std::mem::transmute(NEW);
    let buffer = new(count * std::mem::size_of::<u32>()) as *mut u32;
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
    *begin_pointer = buffer as usize;
    *end_pointer = buffer as usize + count * std::mem::size_of::<u32>();
    *capacity_pointer = *end_pointer;
}

/// Appends a stats record to the game's `StatListBase` vector and returns its
/// index, or `None` when the vector could not be grown.
unsafe fn append_stat(record: usize) -> Option<i32> {
    let base_pointer = STAT_LIST_BASE as *mut usize;
    let end_pointer = STAT_LIST_END as *mut usize;
    let capacity_pointer = STAT_LIST_CAPACITY as *mut usize;

    let base = *base_pointer;
    let end = *end_pointer;
    let capacity = *capacity_pointer;
    if base == 0 {
        return None;
    }
    let count = if end <= base {
        0
    } else {
        (end - base) / std::mem::size_of::<usize>()
    };

    if end < capacity {
        *(end as *mut usize) = record;
        *end_pointer = end + std::mem::size_of::<usize>();
        return Some(count as i32);
    }

    let capacity_count = if base == 0 || capacity <= base {
        0
    } else {
        (capacity - base) / std::mem::size_of::<usize>()
    };
    let new_capacity = capacity_count.max(count).max(8) * 2;
    let new: NewFn = std::mem::transmute(NEW);
    let new_base = new(new_capacity * std::mem::size_of::<usize>());
    if new_base == 0 {
        return None;
    }
    if base != 0 && count != 0 {
        std::ptr::copy_nonoverlapping(
            base as *const u8,
            new_base as *mut u8,
            count * std::mem::size_of::<usize>(),
        );
    }
    if base != 0 {
        let free: FreeFn = std::mem::transmute(FREE);
        free(base as *mut std::ffi::c_void);
    }
    *((new_base + count * std::mem::size_of::<usize>()) as *mut usize) = record;
    *base_pointer = new_base;
    *end_pointer = new_base + (count + 1) * std::mem::size_of::<usize>();
    *capacity_pointer = new_base + new_capacity * std::mem::size_of::<usize>();
    Some(count as i32)
}

/// Sets a clone's per-mode state: free/unlimited in the editor (or a no-gating
/// menu), and the private resource stats only while it is offered by a shop.
unsafe fn apply_clone_state(clone: usize, editor: bool, no_gating: bool) {
    let free_mode = editor || no_gating;
    set_i32(
        clone,
        BODY_COUNT,
        if free_mode { UNLIMITED_COUNT } else { 1 } as i32,
    );
    set_u8(clone, BODY_IS_BLUEPRINT, u8::from(!free_mode));
    if let Some(stats) = clone_stats().get(&clone).copied() {
        set_i32(
            clone,
            BODY_STATS_INDEX,
            if free_mode {
                stats.original
            } else {
                stats.private
            },
        );
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
        restore_oid(&saved, body);
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

unsafe fn contains_child(node: usize, body: usize) -> bool {
    if node == 0 || body == 0 {
        return false;
    }
    let sentinel = *((node + NODE_CHILDREN) as *const usize);
    if sentinel == 0 {
        return false;
    }
    let mut entry = *(sentinel as *const usize);
    let mut steps = 0usize;
    while entry != sentinel && steps < 8192 {
        steps += 1;
        let payload = *((entry + NODE_CHILD_PAYLOAD) as *const usize);
        if payload == body {
            return true;
        }
        entry = *(entry as *const usize);
    }
    false
}

/// `HandleClick`'s `CALL ClickFunc` replacement.
unsafe extern "C" fn click(context: usize, gui: usize, button: u32, value: f32) {
    let original: ClickFn = std::mem::transmute(CLICK_FUNC);
    // Only the editor attaches a palette button to the ship; the city shop's
    // list buys its rows. Renaming in the shop would make a bought ITEM_PAPERS
    // bar come back as HULL PARTS, so guard the editor's left click only.
    let editor = *(LOAD_CONSTRUCTOR_OR_EDITOR as *const u8) != 0;
    if button != 1 || !editor {
        original(context, gui, button, value);
        return;
    }
    let saved = begin_click_rename();
    original(context, gui, button, value);
    end_click_rename(&saved);
}

unsafe fn begin_click_rename() -> Vec<(usize, SavedOid)> {
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

    let sentinel = *((node + NODE_CHILDREN) as *const usize);
    if sentinel == 0 {
        return saved;
    }

    let mut entry = *(sentinel as *const usize);
    let mut steps = 0usize;
    while entry != sentinel && steps < 8192 {
        steps += 1;
        let payload = *((entry + NODE_CHILD_PAYLOAD) as *const usize);
        if payload != 0 {
            if let Some(oid) = body_oid(payload) {
                if is_custom_bar(&oid) {
                    if let Some(save) = rename_to_hull(payload) {
                        saved.push((payload, save));
                        if saved.len() >= MAX_CLICK_RENAMES {
                            break;
                        }
                    }
                }
            }
        }
        entry = *(entry as *const usize);
    }

    saved
}

unsafe fn end_click_rename(saved: &[(usize, SavedOid)]) {
    for (body, save) in saved {
        restore_oid(save, *body);
    }
}

/// `ObjectList_BuyPart`'s `CALL _BuyPartShop` replacement.
///
/// The private stats already carry the menu resource, so the normal purchase
/// works untouched. This wrapper only retries with the city shop node when the
/// flyout's linked node is a palette library that does not contain the bar.
unsafe extern "C" fn buy(part: usize, count: i32, target: usize) -> usize {
    let original: BuyFn = std::mem::transmute(BUY_PART_SHOP);
    let result = original(part, count, target);
    if result != 0 {
        return result;
    }

    let oid = body_oid(part).unwrap_or_default();
    let Some(bar) = find_menu_bar(&oid) else {
        return result;
    };
    if node_has_oid(target, &bar) {
        return result;
    }

    let city = *(CURRENT_LOCATION as *const usize);
    let shop = if city == 0 {
        0
    } else {
        *((city + SHOP_NODE_OFFSET) as *const usize)
    };
    if shop == 0 || shop == target {
        return result;
    }
    original(part, count, shop)
}

fn find_menu_bar(oid: &[u8]) -> Option<Vec<u8>> {
    menus()
        .iter()
        .find(|menu| menu.parts.iter().any(|part| part.oid == oid))
        .map(|menu| menu.bar.clone())
}

unsafe fn node_has_oid(node: usize, oid: &[u8]) -> bool {
    if node == 0 {
        return false;
    }
    let sentinel = *((node + NODE_CHILDREN) as *const usize);
    if sentinel == 0 {
        return false;
    }
    let mut entry = *(sentinel as *const usize);
    let mut steps = 0usize;
    while entry != sentinel && steps < 8192 {
        steps += 1;
        let payload = *((entry + NODE_CHILD_PAYLOAD) as *const usize);
        if payload != 0 {
            if let Some(candidate) = body_oid(payload) {
                if candidate == oid {
                    return true;
                }
            }
        }
        entry = *(entry as *const usize);
    }
    false
}

unsafe fn get_stats_by_index(index: i32) -> usize {
    if index < 0 {
        return 0;
    }
    let base = *(STAT_LIST_BASE as *const usize);
    let end = *(STAT_LIST_END as *const usize);
    if base == 0 || end <= base {
        return 0;
    }
    let count = (end - base) / std::mem::size_of::<usize>();
    if index as usize >= count {
        return 0;
    }
    *((base + index as usize * std::mem::size_of::<usize>()) as *const usize)
}

unsafe fn get_stats(part: usize) -> usize {
    get_stats_by_index(*((part + BODY_STATS_INDEX) as *const i32))
}

/// Returns how many resource entries the part's shared stats currently have.
unsafe fn resource_count(part: usize) -> u32 {
    let stats = get_stats(part);
    if stats == 0 {
        return 0;
    }
    let begin = *((stats + STATS_RESOURCES_BEGIN) as *const usize);
    let end = *((stats + STATS_RESOURCES_END) as *const usize);
    if end <= begin {
        return 0;
    }
    ((end - begin) / std::mem::size_of::<u32>()) as u32
}

/// Returns the part's money price (`Stats.m_price`).
unsafe fn bar_price(part: usize) -> i32 {
    let stats = get_stats(part);
    if stats == 0 {
        return 0;
    }
    *((stats + 0x88) as *const i32)
}

fn find_menu(oid: &[u8], augmentation: bool) -> Option<usize> {
    menus()
        .iter()
        .position(|menu| menu.vanilla_slot.is_some() == augmentation && menu.bar == oid)
}

fn is_custom_bar(oid: &[u8]) -> bool {
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

unsafe fn create_node(name: &[u8]) -> usize {
    let new: NewFn = std::mem::transmute(NEW);
    let raw = new(NODE_SIZE);
    if raw == 0 {
        return 0;
    }
    let constructor: NodeConstructorFn = std::mem::transmute(NODE_CONSTRUCTOR);
    let node = constructor(raw);
    if node == 0 {
        return 0;
    }
    let copy: CopyStringFn = std::mem::transmute(COPY_STRING);
    copy(node + NODE_NAME, name.as_ptr() as *const c_char, name.len());
    node
}

unsafe fn define_part(oid: &CString, node: usize, count: i32) -> usize {
    let all_parts = *(ALL_PARTS as *const usize);
    if all_parts == 0 || node == 0 {
        return 0;
    }
    let define: DefinePartFn = std::mem::transmute(DEFINE_PART);
    define(all_parts, oid.as_ptr(), node, count)
}

unsafe fn body_oid(body: usize) -> Option<Vec<u8>> {
    if body == 0 {
        return None;
    }
    let base = body as *const u8;
    let capacity = (base.add(BODY_OID_CAPACITY) as *const u64).read();
    let text = if capacity < 16 {
        base.add(BODY_OID)
    } else {
        (base.add(BODY_OID) as *const *const u8).read()
    };
    if text.is_null() {
        return None;
    }
    let length = (base.add(BODY_OID_LENGTH) as *const u64).read() as usize;
    if length == 0 || length > 256 {
        return None;
    }
    Some(slice::from_raw_parts(text, length).to_vec())
}

unsafe fn rename_to_hull(body: usize) -> Option<SavedOid> {
    let base = body as *const u8;
    let capacity = (base.add(BODY_OID_CAPACITY) as *const u64).read();
    if capacity < 10 {
        log::warn!("Part menus: a config bar id is too short to be renamed, click guard skipped");
        return None;
    }
    let text = if capacity < 16 {
        body + BODY_OID
    } else {
        (base.add(BODY_OID) as *const *const u8).read() as usize
    };
    if text == 0 {
        return None;
    }

    let mut bytes = [0u8; 10];
    std::ptr::copy_nonoverlapping(text as *const u8, bytes.as_mut_ptr(), 10);
    let length = (base.add(BODY_OID_LENGTH) as *const u64).read();

    std::ptr::copy_nonoverlapping(b"ITEM_HULL\0".as_ptr(), text as *mut u8, 10);
    (base.add(BODY_OID_LENGTH) as *mut u64).write(9);

    Some(SavedOid {
        text,
        bytes,
        length,
    })
}

unsafe fn restore_oid(saved: &SavedOid, body: usize) {
    std::ptr::copy_nonoverlapping(saved.bytes.as_ptr(), saved.text as *mut u8, 10);
    ((body as *const u8).add(BODY_OID_LENGTH) as *mut u64).write(saved.length);
}

/// Returns the length of an `EscadraName` record (the resource table layout).
unsafe fn record_name(record: usize) -> Option<Vec<u8>> {
    if record == 0 {
        return None;
    }
    let length = *((record + 0x10) as *const u64) as usize;
    if length > 256 {
        return None;
    }
    let capacity = *((record + 0x18) as *const u64);
    let text = if capacity < 16 {
        record
    } else {
        *(record as *const usize)
    };
    if text == 0 || length == 0 {
        return None;
    }
    Some(slice::from_raw_parts(text as *const u8, length).to_vec())
}

unsafe fn write_resource_name(id: u32, name: &[u8]) -> bool {
    let record = RESOURCE_TABLE + id as usize * 0x40;
    if let Some(existing) = record_name(record) {
        if existing == name {
            return true;
        }
        log::error!(
            "Part menus: resource id {id} is already named \"{}\", cannot use \"{}\"",
            display(&existing),
            display(name)
        );
        return false;
    }

    if name.len() <= 15 {
        let text = record as *mut u8;
        std::ptr::copy_nonoverlapping(name.as_ptr(), text, name.len());
        text.add(name.len()).write(0);
        ((record + 0x10) as *mut u64).write(name.len() as u64);
        ((record + 0x18) as *mut u64).write(15);
    } else {
        let new: NewFn = std::mem::transmute(NEW);
        let buffer = new(name.len() + 1);
        if buffer == 0 {
            return false;
        }
        std::ptr::copy_nonoverlapping(name.as_ptr(), buffer as *mut u8, name.len());
        (buffer as *mut u8).add(name.len()).write(0);
        (record as *mut usize).write(buffer);
        ((record + 0x10) as *mut u64).write(name.len() as u64);
        ((record + 0x18) as *mut u64).write(name.len() as u64);
    }
    true
}

unsafe fn find_child(node: usize, oid: &[u8]) -> Option<usize> {
    let sentinel = *((node + NODE_CHILDREN) as *const usize);
    if sentinel == 0 {
        return None;
    }
    let mut entry = *(sentinel as *const usize);
    let mut steps = 0usize;
    while entry != sentinel && steps < 8192 {
        steps += 1;
        let payload = *((entry + NODE_CHILD_PAYLOAD) as *const usize);
        if payload != 0 {
            if let Some(candidate) = body_oid(payload) {
                if candidate == oid {
                    return Some(payload);
                }
            }
        }
        entry = *(entry as *const usize);
    }
    None
}

/// Marks the bar's prototype in `ALL_PARTS` as loot.
///
/// Vanilla resource bars (`ITEM_HULL`, ...) have `is_loot=true` in
/// `parts.seria`. A custom bar oid usually does not, and the game then treats a
/// bought bar as a placeable body, which floats when dragged out of the
/// inventory. The bar clone gets the flag too, but the placement path looks the
/// prototype up by oid.
unsafe fn mark_prototype_loot(oid: &[u8]) {
    let all_parts = *(ALL_PARTS as *const usize);
    if all_parts == 0 {
        return;
    }
    match find_child(all_parts, oid) {
        Some(prototype) => {
            if *((prototype + BODY_IS_LOOT) as *const u8) == 0 {
                set_u8(prototype, BODY_IS_LOOT, 1);
                log::debug!("Part menus: marked prototype \"{}\" as loot", display(oid));
            }
        }
        None => log::warn!(
            "Part menus: bar prototype \"{}\" was not found in ALL_PARTS",
            display(oid)
        ),
    }
}

unsafe fn set_u8(body: usize, offset: usize, value: u8) {
    ((body + offset) as *mut u8).write(value);
}

unsafe fn set_i32(body: usize, offset: usize, value: i32) {
    ((body + offset) as *mut i32).write(value);
}

fn display(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
