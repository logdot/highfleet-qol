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

    _unknown_b8: [u8; 0xec],

    /// Index of the selected ammo definition in the live ammo vector.
    pub gun_ammo_index: i32,
    _unknown_1a8: [u8; 8],
}

const _: () = {
    assert!(std::mem::offset_of!(AircraftLogic, gun_reload_time) == 0xa0);
    assert!(std::mem::offset_of!(AircraftLogic, gun_rate) == 0xa8);
    assert!(std::mem::offset_of!(AircraftLogic, gun_load) == 0xac);
    assert!(std::mem::offset_of!(AircraftLogic, gun_max_load) == 0xb0);
    assert!(std::mem::offset_of!(AircraftLogic, gun_reserve_ammo) == 0xb4);
    assert!(std::mem::offset_of!(AircraftLogic, gun_ammo_index) == 0x1a4);
    assert!(std::mem::size_of::<AircraftLogic>() == 0x1b0);
    assert!(std::mem::align_of::<AircraftLogic>() == 8);
};
