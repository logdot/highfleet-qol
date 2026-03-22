use crate::patchy::Patch;

/// In v1.151, gun blocking already exists in the game.
/// This function NOPs out the blocking check to allow guns to fire through own ship.
#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_blocking() {
    let address: usize = 0x14003314d;
    let size: usize = 6;

    let data = vec![0x90; size]; // NOP instructions
    let p = Patch::overwrite(address, &data);
    std::mem::forget(p);
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
    use mmap_rs::MmapOptions;

    // FireGun addresses in v1.163
    const INJECTION_ADDR: usize = 0x140032f22;
    const EXIT_0_ADDR: usize = 0x140032ef0;
    const RETURN_ADDR: usize = 0x140032f29;
    const OVERWRITE_SIZE: usize = 7; // SUBSS XMM1,XMM7 (4) + COMISS XMM6,XMM1 (3)

    let fn_ptr = is_gun_blocked as usize;

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
    //   save XMM1 + shadow space
    //   call is_gun_blocked(RDI)
    //   restore XMM1
    //   if blocked → JMP EXIT_0 (0x140032ef0)
    //   else       → replay overwritten instructions, JMP 0x140032f29
    let mut cave: Vec<u8> = Vec::with_capacity(64);

    // SUB RSP, 0x30  (0x10 XMM1 save + 0x20 shadow space)
    cave.extend_from_slice(&[0x48, 0x83, 0xEC, 0x30]);
    // MOVDQU [RSP+0x20], XMM1
    cave.extend_from_slice(&[0xF3, 0x0F, 0x7F, 0x4C, 0x24, 0x20]);
    // MOV RCX, RDI
    cave.extend_from_slice(&[0x48, 0x89, 0xF9]);
    // FF 15 02 00 00 00   CALL [RIP+2]
    // EB 08               JMP +8  (skip over 8-byte pointer)
    // <8 bytes>           absolute function pointer
    cave.extend_from_slice(&[0xFF, 0x15, 0x02, 0x00, 0x00, 0x00]);
    cave.extend_from_slice(&[0xEB, 0x08]);
    cave.extend_from_slice(&fn_ptr.to_le_bytes());
    // MOVDQU XMM1, [RSP+0x20]
    cave.extend_from_slice(&[0xF3, 0x0F, 0x6F, 0x4C, 0x24, 0x20]);
    // ADD RSP, 0x30
    cave.extend_from_slice(&[0x48, 0x83, 0xC4, 0x30]);
    // TEST AL, AL
    cave.extend_from_slice(&[0x84, 0xC0]);
    // JNZ blocked  (forward jump over 4+3+5 = 12 bytes)
    cave.extend_from_slice(&[0x75, 0x0C]);

    // --- Not blocked: replay overwritten instructions ---
    // SUBSS XMM1, XMM7
    cave.extend_from_slice(&[0xF3, 0x0F, 0x5C, 0xCF]);
    // COMISS XMM6, XMM1
    cave.extend_from_slice(&[0x0F, 0x2F, 0xF1]);
    // JMP rel32 → RETURN_ADDR (placeholder, fixed up below)
    let jmp_back_off = cave.len();
    cave.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]);

    // --- Blocked: jump to EXIT_0 ---
    let jmp_exit_off = cave.len();
    cave.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]);

    // --- Allocate executable memory near FireGun ---
    let cave_addr = match crate::patchy::search_memory_cave(INJECTION_ADDR) {
        Some(addr) => addr,
        None => {
            log::error!("gun_blocking: no memory cave found near FireGun");
            return;
        }
    };

    let mut mmap = match MmapOptions::new(MmapOptions::page_size())
        .unwrap()
        .with_address(cave_addr)
        .map_mut()
    {
        Ok(m) => m,
        Err(e) => {
            log::error!("gun_blocking: mmap allocation failed: {e}");
            return;
        }
    };

    let cave_base = mmap.as_mut_ptr() as usize;

    // Fix up JMP back → RETURN_ADDR
    let src_back = cave_base + jmp_back_off + 5;
    let rel_back = (RETURN_ADDR as isize) - (src_back as isize);
    cave[jmp_back_off + 1..jmp_back_off + 5].copy_from_slice(&(rel_back as i32).to_le_bytes());

    // Fix up JMP exit → EXIT_0_ADDR
    let src_exit = cave_base + jmp_exit_off + 5;
    let rel_exit = (EXIT_0_ADDR as isize) - (src_exit as isize);
    cave[jmp_exit_off + 1..jmp_exit_off + 5].copy_from_slice(&(rel_exit as i32).to_le_bytes());

    // Write trampoline into the cave
    std::ptr::copy_nonoverlapping(cave.as_ptr(), mmap.as_mut_ptr(), cave.len());

    // Make executable (consume the MmapMut, get back an immutable exec Mmap)
    let mmap = mmap.make_exec().expect("gun_blocking: make_exec failed");

    // --- Patch the injection site ---
    // E9 <rel32>  JMP cave_base
    // 90 90       NOP NOP  (pad remaining 2 of 7 overwritten bytes)
    let mut patch_bytes: Vec<u8> = Vec::with_capacity(OVERWRITE_SIZE);
    patch_bytes.push(0xE9);
    let jmp_to_cave = (cave_base as isize) - ((INJECTION_ADDR + 5) as isize);
    patch_bytes.extend_from_slice(&(jmp_to_cave as i32).to_le_bytes());
    patch_bytes.extend(std::iter::repeat_n(0x90, OVERWRITE_SIZE - 5));

    let p = Patch::overwrite(INJECTION_ADDR, &patch_bytes);

    // Leak both allocations so they live for the process lifetime
    std::mem::forget(p);
    std::mem::forget(mmap);

    log::info!(
        "gun_blocking: trampoline installed at {INJECTION_ADDR:#x} → cave at {cave_base:#x}"
    );
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
