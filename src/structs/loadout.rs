use highfleet::general::EscadraString;
use serde::{Deserialize, Serialize};

use crate::structs::cvec::CVec;

#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents an individual munition on a plane
pub struct ItemMunition {
    /// Name of the item.
    pub name: EscadraString,
    /// How many of this item a plane can carry.
    pub count: u32,
    #[serde(skip)]
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
/// Represents a loadout object inside a Tll
pub struct Loadout {
    pub plane_loadout: EscadraString,
    pub generic_loadout: EscadraString,
    /// Array of items.
    pub items: CVec<ItemMunition>,
    pub launch_loadout_weight: u32,
    pub has_gun37mm: bool,
    #[serde(skip)]
    pub _padding: [u8; 3],
}
