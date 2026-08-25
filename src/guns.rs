use patchy::{Patch, Trampoline};

/// In v1.151, gun blocking already exists in the game.
/// This function NOPs out the blocking check to allow guns to fire through own ship.
#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_blocking() {
    let address: usize = 0x14003314d;
    let size: usize = 6;

    let data = vec![0x90; size]; // NOP instructions
    Patch::overwrite(address, &data);
}

/// Gun blocking is already absent in v1.163, so "unblocking" is a no-op.
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
pub unsafe fn patch_sector_blocking() {}

/// In v1.151, gun blocking is native — nothing to restore.
#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_restoration() {}

/// Restores the gun-blocking sector check into v1.163's FireGun function.
///
/// In v1.151, FireGun contained logic that checked a 360-float "sectors" array
/// on each gun body to determine if the gun's firing arc was blocked by its own
/// ship. This check was removed in v1.163. This patch re-implements it by:
///
/// 1. Writing an `is_gun_blocked` function in Rust
/// 2. Allocating a code cave near FireGun
/// 3. Injecting a trampoline at the charge-decrement point that calls the Rust
///    function and conditionally skips firing if blocked
#[cfg(any(feature = "1_163", not(any(feature = "1_151", feature = "1_163"))))]
pub unsafe fn patch_sector_restoration() {
    // FireGun addresses in v1.163
    const INJECTION_ADDR: usize = 0x140032f22;
    const EXIT_0_ADDR: usize = 0x140032ef0;
    const RETURN_ADDR: usize = 0x140032f29;
    const ORIGINAL_BYTES: [u8; 7] = [
        0xF3, 0x0F, 0x5C, 0xCF, // SUBSS XMM1, XMM7
        0x0F, 0x2F, 0xF1, // COMISS XMM6, XMM1
    ];

    // --- Build the code cave trampoline ---
    //
    // At the injection point (0x140032f22), register state is:
    //   RDI  = param_1 (Body pointer, non-volatile)
    //   XMM1 = fFireCharge (volatile — must save across call)
    //   XMM6 = 0.0  (non-volatile)
    //   XMM7 = 1.0  (non-volatile)
    //   RSP  ≡ 0 mod 16
    //
    // Trampoline logic:
    //   save all volatile Windows-x64 registers
    //   call is_gun_blocked(RDI)
    //   restore all volatile registers
    //   if blocked → JMP EXIT_0 (0x140032ef0)
    //   else       → replay overwritten instructions, JMP 0x140032f29
    let mut cave = Trampoline::new();
    let blocked = cave.new_label();
    cave.preserved_call_and_compare_al(
        is_gun_blocked as *const (),
        &[0x48, 0x89, 0xF9], // MOV RCX, RDI
        0,
    );
    cave.jump_if_not_zero(blocked);
    cave.bytes(&ORIGINAL_BYTES);
    cave.relative_jump(RETURN_ADDR);
    cave.bind(blocked)
        .expect("gun-blocking trampoline label was bound twice");
    cave.relative_jump(EXIT_0_ADDR);

    let p = Patch::detour_trampoline(INJECTION_ADDR, ORIGINAL_BYTES.len(), cave);
    let cave_base = p
        .trampoline_address()
        .expect("gun-blocking detour has no trampoline");

    log::info!("gun_blocking: trampoline prepared at {INJECTION_ADDR:#x} → cave at {cave_base:#x}");
}

// ---------------------------------------------------------------------------
// Gun-blocking sector check (mirrors v1.151 FireGun logic)
// ---------------------------------------------------------------------------

/// Body struct field offsets for v1.163.
///
/// Mapped from v1.151 by cross-referencing disassembly patterns.
/// Fields below 0x4F0 are unchanged; the sectors vector shifted +0x10
/// due to a new int-vector inserted at 0x4F0.
mod body {
    pub const M_CODE: usize = 0x028;
    pub const MASTER_NODE: usize = 0x088;
    pub const OWNER_NODE: usize = 0x0B8;
    pub const ANGLE: usize = 0x138;
    pub const PART_INDEX: usize = 0x188;
    pub const SECTORS_BEGIN: usize = 0x508;
    pub const SECTORS_END: usize = 0x510;
}

/// ShipPart struct field offsets for v1.163.
mod ship_part {
    pub const MDL_SECTORS_TYPE: usize = 0x128;
}

const TAU: f32 = 6.2831855;
const SECTOR_COUNT: usize = 360;
const CODE_BODY: u8 = 0x0F;

/// Address of `GetStats` in v1.163 (equivalent to v1.151's `GetShipPart`).
const GET_STATS_ADDR: usize = 0x140281e00;

type GetStatsFn = unsafe extern "C" fn(i32) -> *const u8;

/// Determines whether a gun body's firing arc is blocked by its own ship.
///
/// Reimplements the sector-check algorithm from v1.151's FireGun:
/// 1. Look up the gun's ShipPart and verify it carries sector data
/// 2. Validate the sectors float-array has exactly 360 entries
/// 3. Walk the body hierarchy to find the root body
/// 4. Compute the gun's angle relative to the root body
/// 5. Normalize to \[0, 2pi) and map to a sector index 0..359
/// 6. Return true if that sector is blocked (value == 0.0)
unsafe extern "C" fn is_gun_blocked(gun: *const u8) -> bool {
    // 1. Get ShipPart via part_index
    let part_index = *(gun.add(body::PART_INDEX) as *const i32);
    let get_stats: GetStatsFn = std::mem::transmute(GET_STATS_ADDR);
    let part = get_stats(part_index);
    if part.is_null() {
        return false;
    }

    // 2. Part must define sector data
    if *(part.add(ship_part::MDL_SECTORS_TYPE) as *const i32) == 0 {
        return false;
    }

    // 3. Gun must have a parent body
    let owner = *(gun.add(body::OWNER_NODE) as *const *const u8);
    if owner.is_null() {
        return false;
    }

    // 4. Sectors array must contain exactly 360 floats (0x5A0 bytes)
    let sectors_begin = *(gun.add(body::SECTORS_BEGIN) as *const *const f32);
    let sectors_end = *(gun.add(body::SECTORS_END) as *const *const f32);
    if sectors_begin.is_null() {
        return false;
    }
    let byte_span = (sectors_end as isize - sectors_begin as isize) & !3isize;
    if byte_span != (SECTOR_COUNT * std::mem::size_of::<f32>()) as isize {
        return false;
    }

    // 5. Walk master chain to root Body
    let mut root = owner;
    loop {
        let cursor = *(root.add(body::MASTER_NODE) as *const *const u8);
        if cursor.is_null() || (*cursor.add(body::M_CODE) & 0x0F) != CODE_BODY {
            break;
        }
        root = cursor;
    }

    // 6. Relative angle: gun minus root
    let gun_angle = *(gun.add(body::ANGLE) as *const f32);
    let root_angle = *(root.add(body::ANGLE) as *const f32);
    let mut rel = gun_angle - root_angle;

    // 7. Normalize into [0, TAU)
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

    // 8. Map to sector index and check
    let idx = ((rel / TAU) * SECTOR_COUNT as f32) as i32;
    if idx >= 0 && (idx as u64) < SECTOR_COUNT as u64 && *sectors_begin.offset(idx as isize) == 0.0
    {
        return true; // BLOCKED
    }

    false
}
