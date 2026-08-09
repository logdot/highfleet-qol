use highfleet::general::EscadraString;
use serde::Serialize;

use crate::structs::cvec::CVec;

#[repr(C)]
#[derive(Debug, Clone, Serialize)]
/// Represents an individual munition in the game's native layout.
pub struct GameItemMunition {
    /// Name of the item.
    pub name: EscadraString,
    /// How many of this item a plane can carry.
    pub count: u32,
    #[serde(skip)]
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Serialize)]
/// Represents a loadout object in the game's native TLL layout.
pub struct GameLoadout {
    pub oid: EscadraString,
    pub icon: EscadraString,
    pub vec_parts: CVec<GameItemMunition>,
    pub launch_loadout_weight: u32,
    pub has_gun37mm: bool,
    #[serde(skip)]
    pub _padding: [u8; 3],
}

const _: () = assert!(std::mem::size_of::<GameItemMunition>() == 0x28);
const _: () = assert!(std::mem::size_of::<GameLoadout>() == 0x60);
