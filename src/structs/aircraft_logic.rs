/// Game-owned aircraft behavior state shared by HighFleet 1.151 and 1.163.
///
/// Only the fields currently understood by QOL are exposed. The byte arrays preserve
/// the verified native layout while leaving the remaining fields uninterpreted.
#[repr(C, align(8))]
pub struct AircraftLogic {
    _unknown_000: [u8; 0xa0],

    /// Time in seconds used to reload the aircraft gun.
    pub gun_reload_time: f32,
    _unknown_a4: [u8; 4],

    /// Fire-rate timing value copied from the selected ammo definition.
    pub gun_rate: f32,
    /// Number of rounds currently loaded in the gun.
    pub gun_load: i32,
    /// Maximum number of rounds that can be loaded in the gun.
    pub gun_max_load: i32,
    /// Number of gun rounds available outside the current load.
    pub gun_reserve_ammo: i32,

    _unknown_b8: [u8; 0x68],

    /// Bombs the aircraft currently carries, as the battle panel counts them.
    pub bombs: CraftStock,
    /// Rockets the aircraft currently carries.
    pub rockets: CraftStock,
    /// Air-to-air missiles the aircraft currently carries.
    pub aa_missiles: CraftStock,

    _unknown_150: [u8; 0x54],

    /// Index of the selected ammo definition in the live ammo vector.
    pub gun_ammo_index: i32,
    _unknown_1a8: [u8; 8],
}

/// One counted load inside [`AircraftLogic`]: `0x10` bytes with its element count at `+8`.
///
/// The panel reads only the count; the vector's remaining state is not interpreted here.
#[repr(C)]
pub struct CraftStock {
    _unknown_000: [u8; 8],
    /// Number of rounds of this kind the aircraft carries.
    pub count: i32,
    _unknown_00c: [u8; 4],
}

const _: () = {
    assert!(std::mem::offset_of!(AircraftLogic, gun_reload_time) == 0xa0);
    assert!(std::mem::offset_of!(AircraftLogic, gun_rate) == 0xa8);
    assert!(std::mem::offset_of!(AircraftLogic, gun_load) == 0xac);
    assert!(std::mem::offset_of!(AircraftLogic, gun_max_load) == 0xb0);
    assert!(std::mem::offset_of!(AircraftLogic, gun_reserve_ammo) == 0xb4);
    assert!(std::mem::offset_of!(AircraftLogic, bombs) == 0x120);
    assert!(std::mem::offset_of!(AircraftLogic, rockets) == 0x130);
    assert!(std::mem::offset_of!(AircraftLogic, aa_missiles) == 0x140);
    assert!(std::mem::offset_of!(AircraftLogic, gun_ammo_index) == 0x1a4);
    assert!(std::mem::size_of::<AircraftLogic>() == 0x1b0);
    assert!(std::mem::align_of::<AircraftLogic>() == 8);
    assert!(std::mem::offset_of!(CraftStock, count) == 8);
    assert!(std::mem::size_of::<CraftStock>() == 0x10);
};
