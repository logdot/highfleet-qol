//! Opt-in Steam 1.151 initial strategic-missile target distribution prototype.

use std::{cell::Cell, mem::transmute, ptr, slice};

use patchy::{PatchError, PatchSession, ProcessModule, Result, ReturnType, Trampoline};

const INITIALIZER: usize = 0x140241680;
const INITIALIZE_CALL: usize = 0x1402ab0f0;
const SELECT_TARGET: usize = 0x140241d7a;
const CALL_BYTES: [u8; 5] = [0xe8, 0x8b, 0x65, 0xf9, 0xff];
const SELECT_BYTES: [u8; 7] = [0x49, 0x8b, 0x07, 0x48, 0x8b, 0x50, 0x10];
const MAX_CANDIDATES: usize = 4096;

thread_local! {
    // Only the central salvo-spawning CALL activates distribution. No native
    // list pointers survive the initializer's destruction of its temporary list.
    static MISSILE_INDEX: Cell<Option<u32>> = const { Cell::new(None) };
}

pub(crate) unsafe fn patch_missile_targets(session: &mut PatchSession) -> Result {
    if ProcessModule::main()?.base_address() != 0x140000000 {
        return Err(PatchError::ModuleLookup(
            "missile target prototype requires the native 1.151 image base".into(),
        ));
    }
    for (address, expected) in [
        (INITIALIZE_CALL, CALL_BYTES.as_slice()),
        (SELECT_TARGET, SELECT_BYTES.as_slice()),
        // Verify ESI is the salvo ordinal and RCX/RDX are the native arguments.
        (0x1402ab0ea, &[0x48, 0x8b, 0xd7, 0x48, 0x8b, 0xcb][..]),
        (
            0x1402ab0f5,
            &[0xff, 0xc6, 0x3b, 0xb7, 0x48, 0x02, 0x00, 0x00][..],
        ),
    ] {
        let found = slice::from_raw_parts(address as *const u8, expected.len());
        if found != expected {
            log::error!(
                "missile_targets: refusing prototype; unexpected bytes at {address:#x}: {found:02x?}"
            );
            return Err(PatchError::SourceChanged { address });
        }
    }

    let mut initialize = Trampoline::new();
    initialize.bytes(&[0x41, 0x89, 0xf0]); // MOV R8D, ESI
                                           // At the replaced CALL, RSP is already aligned with native shadow space.
    initialize.absolute_call(initialize_missile as *const ());
    initialize.absolute_jump(INITIALIZE_CALL + CALL_BYTES.len());

    let mut select = Trampoline::new();
    // The overwritten MOVs did not change flags. Preserve them too, with an
    // extra eight-byte slot so preserved_call still starts with aligned RSP.
    select.bytes(&[0x9c, 0x48, 0x8d, 0x64, 0x24, 0xf8]); // PUSHFQ; LEA RSP,[RSP-8]
    select.preserved_call(
        select_target as *const (),
        &[0x4c, 0x89, 0xf9], // MOV RCX, R15 (sorted-list sentinel)
        &[],
        ReturnType::Rax,
    );
    select.bytes(&[0x48, 0x8d, 0x64, 0x24, 0x08, 0x9d]); // LEA RSP,[RSP+8]; POPFQ
    select.bytes(&[0x48, 0x89, 0xc2]); // MOV RDX, RAX (selected Body*)
                                       // Resume at the native LEA/CALL SetLink, preserving Link bookkeeping.
    select.absolute_jump(SELECT_TARGET + SELECT_BYTES.len());

    session.detour_trampoline(INITIALIZE_CALL, CALL_BYTES.len(), initialize)?;
    session.detour_trampoline(SELECT_TARGET, SELECT_BYTES.len(), select)?;
    log::warn!("Prepared experimental CV-ranked round-robin missile targets for Steam 1.151; later guidance is unverified");
    Ok(())
}

unsafe extern "C" fn initialize_missile(missile: *mut u8, strategic: *mut u8, index: u32) {
    MISSILE_INDEX.with(|slot| {
        let previous = slot.replace(Some(index));
        let initialize: unsafe extern "C" fn(*mut u8, *mut u8) = transmute(INITIALIZER);
        initialize(missile, strategic);
        slot.set(previous);
    });
}

unsafe fn read_pointer(object: *const u8, offset: usize) -> *const u8 {
    ptr::read_unaligned(object.add(offset).cast())
}

unsafe extern "C" fn select_target(sentinel: *const u8) -> *const u8 {
    if sentinel.is_null() {
        return ptr::null();
    }
    let first = read_pointer(sentinel, 0);
    if first.is_null() || first == sentinel {
        return ptr::null();
    }
    let fallback = read_pointer(first, 0x10);
    let Some(index) = MISSILE_INDEX.with(Cell::get) else {
        return fallback;
    };

    let mut node = first;
    let mut count = 0;
    while node != sentinel {
        if node.is_null() || count == MAX_CANDIDATES {
            log::warn!(
                "missile_targets: invalid/oversized candidate list; retaining native first target"
            );
            return fallback;
        }
        count += 1;
        node = read_pointer(node, 0);
    }
    let rank = index as usize % count;
    node = first;
    for _ in 0..rank {
        node = read_pointer(node, 0);
    }
    let target = read_pointer(node, 0x10);
    log::info!(
        "missile_targets: missile slot {index}, CV rank {rank}/{count}, target {target:p} (native first {fallback:p})"
    );
    target
}
