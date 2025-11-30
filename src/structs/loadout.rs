#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ItemMunition {
    /// Name of the item.
    name: EscadraString,
    /// How many of this item a plane can carry.
    count: u32,
    _padding: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Loadout {
    plane_loadout: EscadraString,
    plane_loadout2: EscadraString,
    generic_loadout: EscadraString,
    /// Array of items.
    items: CVec<ItemMunition>,
    item_count: u32,
    _padding: [u8; 4],
}
