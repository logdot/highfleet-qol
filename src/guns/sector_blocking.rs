use highfleet::v1_163::Body;
use std::f32::consts::TAU;

const SECTOR_COUNT: usize = 360;
const CODE_BODY: u64 = 0x0f;
const MDL_SECTORS_TYPE_OFFSET: usize = 0x128;

#[cfg(not(feature = "1_151"))]
const GET_STATS_ADDR: usize = 0x140281e00;

type GetStatsFn = unsafe extern "C" fn(i32) -> *const u8;

/// Checks borrowed native state without adopting or dropping any game-owned fields.
#[cfg(not(feature = "1_151"))]
pub(super) unsafe extern "C" fn is_gun_blocked(gun: *const Body) -> bool {
    let get_stats: GetStatsFn = std::mem::transmute(GET_STATS_ADDR);
    is_gun_blocked_with(gun, get_stats)
}

unsafe fn is_gun_blocked_with(gun: *const Body, get_stats: GetStatsFn) -> bool {
    if gun.is_null() {
        return false;
    }

    let part = get_stats((*gun).part_index);
    if part.is_null() {
        return false;
    }
    // ShipPart is not modeled yet, so its sector-type flag remains an isolated raw access.
    if *part.add(MDL_SECTORS_TYPE_OFFSET).cast::<i32>() == 0 {
        return false;
    }

    let owner = (*gun).base.owner.target;
    if owner.is_null() || ((*owner).base.base.code & 0x0f) != CODE_BODY {
        return false;
    }

    let sectors = &(*gun).sectors;
    if sectors.len() != SECTOR_COUNT {
        return false;
    }
    let sectors = sectors.as_slice();

    let mut root = owner;
    loop {
        let cursor = (*root).master.target;
        if cursor.is_null() || ((*cursor).base.base.code & 0x0f) != CODE_BODY {
            break;
        }
        root = cursor;
    }

    // Every accepted root has passed the Node code check before this Body cast.
    let mut rel = (*gun).angle - (*root.cast::<Body>()).angle;
    let steps = (rel.abs() / TAU + 0.5).floor();
    let full_turns = if rel >= 0.0 {
        steps as i32
    } else {
        -(steps as i32)
    };
    rel -= full_turns as f32 * TAU;
    if rel < 0.0 {
        rel += TAU;
    }

    let idx = ((rel / TAU) * SECTOR_COUNT as f32) as i32;
    idx >= 0 && (idx as u64) < SECTOR_COUNT as u64 && sectors[idx as usize] == 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use highfleet::{
        general::{CVec, EscadraString, IObject, ISerializable, Link, Node},
        v1_163::Body,
    };
    use std::{
        mem::{offset_of, MaybeUninit},
        ptr,
    };

    const SECTOR_PART_INDEX: i32 = 37;
    const NO_SECTORS_PART_INDEX: i32 = 91;

    #[repr(C, align(4))]
    struct FakeShipPart([u8; MDL_SECTORS_TYPE_OFFSET + 4]);

    const fn fake_part(flag: i32) -> FakeShipPart {
        let mut bytes = [0; MDL_SECTORS_TYPE_OFFSET + 4];
        let flag = flag.to_ne_bytes();
        let mut i = 0;
        while i < flag.len() {
            bytes[MDL_SECTORS_TYPE_OFFSET + i] = flag[i];
            i += 1;
        }
        FakeShipPart(bytes)
    }

    // A negative nonzero flag also verifies that the native field is tested as an i32.
    static SECTOR_PART: FakeShipPart = fake_part(-256);
    static NO_SECTORS_PART: FakeShipPart = fake_part(0);

    unsafe extern "C" fn mock_get_stats(index: i32) -> *const u8 {
        match index {
            SECTOR_PART_INDEX => SECTOR_PART.0.as_ptr(),
            NO_SECTORS_PART_INDEX => NO_SECTORS_PART.0.as_ptr(),
            _ => ptr::null(),
        }
    }

    fn body() -> Body {
        let mut storage = MaybeUninit::<Body>::zeroed();
        let raw = storage.as_mut_ptr();
        unsafe {
            // Initialize all five strings and all six vector headers before creating an owned Body.
            ptr::addr_of_mut!((*raw).base.base.base.class_name).write(EscadraString::new());
            ptr::addr_of_mut!((*raw).base.name).write(EscadraString::new());
            ptr::addr_of_mut!((*raw).oid).write(EscadraString::new());
            ptr::addr_of_mut!((*raw).ship_convertion_error).write(EscadraString::new());
            ptr::addr_of_mut!((*raw).logic_s1).write(EscadraString::new());
            ptr::addr_of_mut!((*raw).routs).write(CVec::empty());
            ptr::addr_of_mut!((*raw).slots).write(CVec::empty());
            ptr::addr_of_mut!((*raw).spasmcode).write(CVec::empty());
            ptr::addr_of_mut!((*raw).logic_editor).write(CVec::empty());
            ptr::addr_of_mut!((*raw).sectors).write(CVec::empty());
            ptr::addr_of_mut!((*raw).sprites).write(CVec::empty());
            let mut body = storage.assume_init();
            body.base.base.base.code = 0x123f;
            body.part_index = SECTOR_PART_INDEX;
            body
        }
    }

    fn sectors(len: usize, value: f32) -> CVec<f32> {
        let mut sectors = CVec::empty();
        for _ in 0..len {
            sectors.insert(value);
        }
        sectors
    }

    fn gun_and_owner() -> (Box<Body>, Box<Body>) {
        let mut owner = Box::new(body());
        let mut gun = Box::new(body());
        gun.base.owner.target = &mut owner.base;
        gun.sectors = sectors(SECTOR_COUNT, 0.0);
        (gun, owner)
    }

    fn blocked(gun: &Body) -> bool {
        unsafe { is_gun_blocked_with(gun, mock_get_stats) }
    }

    #[test]
    fn native_offsets_match_the_blocking_abi() {
        assert_eq!(offset_of!(Body, base), 0);
        assert_eq!(offset_of!(Body, part_index), 0x188);
        assert_eq!(offset_of!(Body, sectors), 0x508);
        assert_eq!(offset_of!(Body, angle), 0x138);
        assert_eq!(offset_of!(Node, master) + offset_of!(Link, target), 0x88);
        assert_eq!(offset_of!(Node, owner) + offset_of!(Link, target), 0xb8);
        assert_eq!(
            offset_of!(Node, base) + offset_of!(ISerializable, base) + offset_of!(IObject, code),
            0x28,
        );
    }

    #[test]
    fn null_gun_is_not_blocked() {
        unsafe { assert!(!is_gun_blocked_with(ptr::null(), mock_get_stats)) };
    }

    #[test]
    fn missing_stats_and_zero_sector_flag_are_not_blocked() {
        let (mut gun, _owner) = gun_and_owner();
        assert!(blocked(&gun));
        for index in [-1, 0, i32::MAX, NO_SECTORS_PART_INDEX] {
            gun.part_index = index;
            assert!(!blocked(&gun), "part index {index}");
        }
    }

    #[test]
    fn missing_owner_is_not_blocked() {
        let mut gun = body();
        gun.sectors = sectors(SECTOR_COUNT, 0.0);
        assert!(!blocked(&gun));
    }

    #[test]
    fn non_body_owner_is_not_cast_to_body() {
        let mut owner = Box::new(body().base);
        owner.base.base.code = 0x123e;
        let mut gun = Box::new(body());
        gun.base.owner.target = &mut *owner;
        gun.sectors = sectors(SECTOR_COUNT, 0.0);
        assert!(!blocked(&gun));
    }

    #[test]
    fn sectors_require_exactly_360_entries() {
        let (mut gun, _owner) = gun_and_owner();
        for len in [0, 1, 359, 361, 720] {
            gun.sectors = sectors(len, 0.0);
            assert!(!blocked(&gun), "sector count {len}");
        }
        gun.sectors = sectors(SECTOR_COUNT, 0.0);
        assert!(blocked(&gun));
    }

    #[test]
    fn only_zero_sector_values_are_blocked() {
        let (mut gun, _owner) = gun_and_owner();
        for (value, expected) in [
            (0.0, true),
            (-0.0, true),
            (1.0, false),
            (-1.0, false),
            (f32::NAN, false),
        ] {
            gun.sectors = sectors(SECTOR_COUNT, value);
            assert_eq!(blocked(&gun), expected, "sector value {value}");
        }
    }

    #[test]
    fn relative_angles_wrap_multiple_turns_and_truncate_sector_indices() {
        let (mut gun, mut owner) = gun_and_owner();
        owner.angle = 0.75;
        for (turns, sector) in [
            (0.0, 45),
            (3.0, 45),
            (-4.0, 45),
            (0.0, 315),
            (5.0, 315),
            (-6.0, 315),
        ] {
            gun.sectors = sectors(SECTOR_COUNT, 1.0);
            unsafe { *gun.sectors.as_mut_ptr().add(sector) = 0.0 };
            let fraction = if sector == 45 {
                45.5 / 360.0
            } else {
                -44.5 / 360.0
            };
            gun.angle = owner.angle + (turns + fraction) * TAU;
            assert!(blocked(&gun), "turns {turns}, sector {sector}");
            unsafe { *gun.sectors.as_mut_ptr().add(sector) = 1.0 };
            assert!(!blocked(&gun));
        }
    }

    #[test]
    fn master_chain_uses_last_body_and_stops_at_non_body_node() {
        let (mut gun, mut owner) = gun_and_owner();
        let mut middle = Box::new(body());
        let mut root = Box::new(body());
        let mut beyond = Box::new(body());
        let mut non_body = Box::new(body().base);
        non_body.base.base.code = 0x432e;
        owner.angle = TAU * 0.125;
        middle.angle = TAU * 0.25;
        root.angle = TAU * 0.5;
        beyond.angle = TAU * 0.75;
        owner.base.master.target = &mut middle.base;
        middle.base.master.target = &mut root.base;
        root.base.master.target = &mut *non_body;
        non_body.master.target = &mut beyond.base;
        gun.angle = root.angle;
        gun.sectors = sectors(SECTOR_COUNT, 1.0);
        unsafe { *gun.sectors.as_mut_ptr() = 0.0 };
        assert!(blocked(&gun));
        root.base.master.target = ptr::null_mut();
        assert!(blocked(&gun));
    }

    #[test]
    fn borrowed_fields_and_raw_pointer_targets_remain_owned_by_fixtures() {
        let (mut gun, mut owner) = gun_and_owner();
        let mut target = Box::new(body());
        target.oid = EscadraString::from("a heap-allocated target identifier");
        let target_ptr = &mut target.base.base as *mut ISerializable;
        gun.routs.insert(target_ptr);
        gun.slots.insert(target_ptr);
        gun.sprites.insert(target_ptr);
        owner.base.master.target = &mut target.base;
        let sector_ptr = gun.sectors.as_ptr();
        for _ in 0..3 {
            assert!(blocked(&gun));
            assert_eq!(gun.sectors.as_ptr(), sector_ptr);
            assert_eq!(gun.routs.as_slice(), &[target_ptr]);
        }
        drop(gun);
        assert_eq!(owner.base.master.target, &mut target.base as *mut Node);
        drop(owner);
        assert_eq!(
            target.oid.get_string(),
            "a heap-allocated target identifier"
        );
        target.sectors.insert(1.0);
        assert_eq!(target.sectors.as_slice(), &[1.0]);
    }
}
