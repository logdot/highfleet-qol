#[cfg(not(feature = "1_151"))]
use patchy::{Condition, ReturnType, Trampoline};
use patchy::{PatchSession, Result};

#[cfg(any(not(feature = "1_151"), test))]
mod sector_blocking;
#[cfg(not(feature = "1_151"))]
use sector_blocking::is_gun_blocked;

/// In v1.151, gun blocking already exists in the game.
/// This function NOPs out the blocking check to allow guns to fire through own ship.
#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_blocking(session: &mut PatchSession) -> Result {
    let address: usize = 0x14003314d;
    let size: usize = 6;

    let data = vec![0x90; size]; // NOP instructions
    session.overwrite(address, &data)?;
    Ok(())
}

/// Gun blocking is already absent in v1.163, so "unblocking" is a no-op.
#[cfg(not(feature = "1_151"))]
pub unsafe fn patch_sector_blocking(_session: &mut PatchSession) -> Result {
    Ok(())
}

/// In v1.151, gun blocking is native — nothing to restore.
#[cfg(feature = "1_151")]
pub unsafe fn patch_sector_restoration(_session: &mut PatchSession) -> Result {
    Ok(())
}

/// Restores the gun-blocking sector check into v1.163's FireGun function.
///
/// In v1.151, FireGun contained logic that checked a 360-float "sectors" array on each gun body to determine if the gun's firing arc was blocked by its own ship.
/// This check was removed in v1.163.
/// This patch re-implements it by:
///
/// 1. Writing an `is_gun_blocked` function in Rust
/// 2. Allocating a code cave near FireGun
/// 3. Injecting a trampoline at the charge-decrement point that calls the Rust function and conditionally skips firing if blocked
#[cfg(not(feature = "1_151"))]
pub unsafe fn patch_sector_restoration(session: &mut PatchSession) -> Result {
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
    cave.preserved_call(
        is_gun_blocked as *const (),
        &[0x48, 0x89, 0xF9], // MOV RCX, RDI
        &[0x84, 0xC0],       // TEST AL, AL
        ReturnType::None,
    );
    cave.jump_if(Condition::NotEqual, blocked);
    cave.bytes(&ORIGINAL_BYTES);
    cave.relative_jump(RETURN_ADDR);
    cave.bind(blocked)
        .expect("gun-blocking trampoline label was bound twice");
    cave.relative_jump(EXIT_0_ADDR);

    let p = session.detour_trampoline(INJECTION_ADDR, ORIGINAL_BYTES.len(), cave)?;
    let cave_base = p
        .trampoline_address()
        .expect("gun-blocking detour has no trampoline");

    log::info!("gun_blocking: trampoline prepared at {INJECTION_ADDR:#x} → cave at {cave_base:#x}");
    Ok(())
}
