use patchy::{Condition, PatchError, PatchSession, ProcessModule, Result, ReturnType, Trampoline};

use super::{damage::INITIAL_HEALTH, native};

// These call sites have at most four register arguments. No stack arguments
// need to be forwarded through Patchy's preserved-call callbacks.
const DRAW_SITE: (usize, [u8; 5]) = (0x12e7e4, [0xe8, 0x87, 0x0b, 0x02, 0x00]);
const STEP_SITE: (usize, [u8; 5]) = (0x28658a, [0xe8, 0x01, 0x57, 0x09, 0x00]);
const CONTACT_SITE: (usize, [u8; 5]) = (0x2865fc, [0xe8, 0x9f, 0xe3, 0xff, 0xff]);
const DTOR_SITE: (usize, [u8; 5]) = (0x174d0, [0x48, 0x89, 0x5c, 0x24, 0x08]);
const OVERLAY_SITE: (usize, [u8; 7]) = (0x166420, [0x48, 0x8b, 0xc4, 0x48, 0x89, 0x58, 0x18]);
const FILTER_SLOT: usize = 0x3692a8;

pub(crate) unsafe fn patch_shootable_gui(session: &mut PatchSession) -> Result {
    let module = ProcessModule::main()?;
    for (rva, expected) in [DRAW_SITE, STEP_SITE, CONTACT_SITE, DTOR_SITE] {
        let address = module.resolve_rva(rva)?;
        if std::slice::from_raw_parts(address as *const u8, expected.len()) != expected {
            return Err(PatchError::SourceChanged { address });
        }
    }
    let overlay = module.resolve_rva(OVERLAY_SITE.0)?;
    if std::slice::from_raw_parts(overlay as *const u8, OVERLAY_SITE.1.len()) != OVERLAY_SITE.1 {
        return Err(PatchError::SourceChanged { address: overlay });
    }
    let filter_slot = module.resolve_rva(FILTER_SLOT)?;
    if (filter_slot as *const usize).read() != module.resolve_rva(0x2858c0)? {
        return Err(PatchError::SourceChanged {
            address: filter_slot,
        });
    }
    if !native::configure(|rva| module.resolve_rva(rva))? {
        return Err(PatchError::SourceChanged {
            address: filter_slot,
        });
    }
    for ((rva, _), callback) in [
        (DRAW_SITE, native::draw_element as *const ()),
        (STEP_SITE, native::step_world as *const ()),
        (CONTACT_SITE, native::process_contact as *const ()),
    ] {
        session.patch_call(
            module.resolve_rva(rva)?,
            callback,
            5,
            false,
            ReturnType::None,
        )?;
    }
    session.overwrite(
        filter_slot,
        &(native::should_collide as *const () as usize).to_le_bytes(),
    )?;

    // Function entries have RSP % 16 == 8, unlike the call sites above.
    let mut destructor = Trampoline::new();
    destructor.bytes(&[0x48, 0x83, 0xec, 0x08]);
    destructor.preserved_call(
        native::forget_element as *const (),
        &[],
        &[],
        ReturnType::None,
    );
    destructor.bytes(&[0x48, 0x83, 0xc4, 0x08]);
    destructor.bytes(&DTOR_SITE.1);
    destructor.relative_jump(module.resolve_rva(DTOR_SITE.0 + 5)?);
    session.detour_trampoline(module.resolve_rva(DTOR_SITE.0)?, 5, destructor)?;

    let mut overlay_gate = Trampoline::new();
    let visible = overlay_gate.new_label();
    overlay_gate.bytes(&[0x48, 0x83, 0xec, 0x08]);
    overlay_gate.preserved_call(
        native::may_draw as *const (),
        &[],
        &[0x84, 0xc0],
        ReturnType::None,
    );
    overlay_gate.bytes(&[0x48, 0x8d, 0x64, 0x24, 0x08]); // preserve TEST's flags
    overlay_gate.jump_if(Condition::NotEqual, visible);
    overlay_gate.bytes(&[0xc3]);
    overlay_gate.bind(visible)?;
    overlay_gate.bytes(&OVERLAY_SITE.1);
    overlay_gate.relative_jump(overlay + OVERLAY_SITE.1.len());
    session.detour_trampoline(overlay, OVERLAY_SITE.1.len(), overlay_gate)?;
    log::info!("Prepared experimental shootable GUI: real GUI fixtures, {INITIAL_HEALTH} HP");
    Ok(())
}
