use highfleet::general::EscadraString;
use serde::Serialize;

use crate::structs::cvec::CVec;

#[repr(C)]
#[derive(Debug, Clone, Serialize)]
#[serde(into = "String")]
/// Represents an individual munition on a plane
pub struct ItemMunition {
    /// Name of the item.
    pub name: EscadraString,
    /// How many of this item a plane can carry.
    pub count: u32,
    _padding: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Serialize)]
/// Represents a loadout object inside a Tll
pub struct Loadout {
    pub plane_loadout: EscadraString,
    pub generic_loadout: EscadraString,
    /// Array of items.
    pub items: CVec<ItemMunition>,
    pub launch_loadout_weight: u32,
    pub has_gun37mm: bool,
    _padding: [u8; 3],
}

impl From<ItemMunition> for String {
    fn from(loadout: ItemMunition) -> Self {
        format!("{} x {}", loadout.name.get_string(), loadout.count)
    }
}
