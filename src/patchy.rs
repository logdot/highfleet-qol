//! Simple patching library for Windows processes.

use core::slice;
use std::{
    collections::BTreeSet,
    ffi::c_void,
    fmt,
    ops::Range,
    sync::{Mutex, MutexGuard, OnceLock},
};

use mmap_rs::{Mmap, MmapMut, MmapOptions};
use windows::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS},
    Threading::GetCurrentProcess,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReturnType {
    None,
    Rax,
    Xmm0,
}

const CALL_BYTES: [u8; 8] = [0xff, 0x15, 0x02, 0x00, 0x00, 0x00, 0xeb, 0x08];
const NEAR_JUMP: u8 = 0xe9;
const TRAMPOLINE_ALIGNMENT: usize = 16;
const NEAR_JUMP_SIZE: usize = 5;

// SUB RSP, 0x8 — used to fix 16-byte stack alignment when push count is odd
const ALIGN_STACK: [u8; 4] = [0x48, 0x83, 0xEC, 0x08];
// ADD RSP, 0x8 — undoes the alignment padding
const UNALIGN_STACK: [u8; 4] = [0x48, 0x83, 0xC4, 0x08];

// PUSH RAX
const SAVE_RAX: [u8; 1] = [0x50];
// MOVDQU [RSP + 0x00], XMM0
const SAVE_XMM0: [u8; 5] = [0xF3, 0x0F, 0x7F, 0x04, 0x24];
const SAVE_REGISTERS: [u8; 44] = [
    // PUSH RCX
    0x51, // PUSH RDX
    0x52, // PUSH R8
    0x41, 0x50, // PUSH R9
    0x41, 0x51, // PUSH R10
    0x41, 0x52, // PUSH R11
    0x41, 0x53, // SUB RSP, 0x60
    0x48, 0x83, 0xEC, 0x60, // MOVDQU [RSP + 0x10], XMM1
    0xF3, 0x0F, 0x7F, 0x4C, 0x24, 0x10, // MOVDQU [RSP + 0x20], XMM2
    0xF3, 0x0F, 0x7F, 0x54, 0x24, 0x20, // MOVDQU [RSP + 0x30], XMM3
    0xF3, 0x0F, 0x7F, 0x5C, 0x24, 0x30, // MOVDQU [RSP + 0x40], XMM4
    0xF3, 0x0F, 0x7F, 0x64, 0x24, 0x40, // MOVDQU [RSP + 0x50], XMM5
    0xF3, 0x0F, 0x7F, 0x6C, 0x24, 0x50,
];

// POP RAX
const LOAD_RAX: [u8; 1] = [0x58];
// MOVDQU XMM0, [RSP + 0x00]
const LOAD_XMM0: [u8; 5] = [0xF3, 0x0F, 0x6F, 0x04, 0x24];
const LOAD_REGISTERS: [u8; 44] = [
    // MOVDQU XMM1, [RSP + 0x10]
    0xF3, 0x0F, 0x6F, 0x4C, 0x24, 0x10, // MOVDQU XMM2, [RSP + 0x20]
    0xF3, 0x0F, 0x6F, 0x54, 0x24, 0x20, // MOVDQU XMM3, [RSP + 0x30]
    0xF3, 0x0F, 0x6F, 0x5C, 0x24, 0x30, // MOVDQU XMM4, [RSP + 0x40]
    0xF3, 0x0F, 0x6F, 0x64, 0x24, 0x40, // MOVDQU XMM5, [RSP + 0x50]
    0xF3, 0x0F, 0x6F, 0x6C, 0x24, 0x50, // ADD RSP, 0x60
    0x48, 0x83, 0xC4, 0x60, // POP R11
    0x41, 0x5B, // POP R10
    0x41, 0x5A, // POP R9
    0x41, 0x59, // POP R8
    0x41, 0x58, // POP RDX
    0x5A, // POP RCX
    0x59,
];

/// A handle describing a patch prepared for installation.
#[allow(dead_code)]
pub struct Patch {
    address: usize,
    size: usize,
    overwritten: Vec<u8>,
    trampoline: Option<CodeAllocation>,
}

#[derive(Clone, Copy)]
struct CodeAllocation {
    address: usize,
}

#[derive(Debug)]
pub(crate) enum PatchError {
    AddressOverflow,
    AlreadyFinalized,
    EmptyPatch,
    InstructionCache {
        address: usize,
        error: String,
    },
    Mapping(String),
    NoMemoryCave {
        hook: usize,
        last_error: Option<String>,
    },
    OverlappingPatch {
        first: usize,
        second: usize,
    },
    Protection {
        address: usize,
        error: String,
    },
    RelativeJumpOutOfRange {
        next_instruction: usize,
        destination: usize,
    },
    SourceChanged {
        address: usize,
    },
    TrampolineTooLarge {
        size: usize,
        capacity: usize,
    },
    UnexpectedTrampolineSize {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddressOverflow => write!(f, "address calculation overflowed"),
            Self::AlreadyFinalized => write!(f, "patch installation has already been finalized"),
            Self::EmptyPatch => write!(f, "a patch must overwrite at least one byte"),
            Self::InstructionCache { address, error } => write!(
                f,
                "unable to flush the instruction cache at {address:#x}: {error}"
            ),
            Self::Mapping(error) => write!(f, "memory mapping failed: {error}"),
            Self::NoMemoryCave { hook, last_error } => {
                write!(f, "no usable trampoline page found near {hook:#x}")?;
                if let Some(error) = last_error {
                    write!(f, ": {error}")?;
                }
                Ok(())
            }
            Self::OverlappingPatch { first, second } => write!(
                f,
                "patch at {second:#x} overlaps the patch prepared at {first:#x}"
            ),
            Self::Protection { address, error } => {
                write!(f, "unable to change protection at {address:#x}: {error}")
            }
            Self::RelativeJumpOutOfRange {
                next_instruction,
                destination,
            } => write!(
                f,
                "relative jump from {next_instruction:#x} to {destination:#x} exceeds 32 bits"
            ),
            Self::SourceChanged { address } => write!(
                f,
                "patch source at {address:#x} changed while patches were being prepared"
            ),
            Self::TrampolineTooLarge { size, capacity } => write!(
                f,
                "trampoline requires {size} bytes but a page holds only {capacity}"
            ),
            Self::UnexpectedTrampolineSize { expected, actual } => write!(
                f,
                "trampoline builder produced {actual} bytes instead of {expected}"
            ),
        }
    }
}

impl std::error::Error for PatchError {}

#[derive(Default)]
struct PatchSession {
    pages: Vec<MutableCodePage>,
    pending: Vec<PendingPatch>,
}

struct MutableCodePage {
    mapping: MmapMut,
    used: usize,
}

struct PendingPatch {
    address: usize,
    overwritten: Vec<u8>,
    replacement: Vec<u8>,
}

struct PatchRuntime {
    pages: Vec<Mmap>,
    patches: Vec<PendingPatch>,
}

struct PatchManager {
    session: Option<PatchSession>,
    runtime: Option<PatchRuntime>,
}

impl PatchManager {
    fn new() -> Self {
        Self {
            session: Some(PatchSession::default()),
            runtime: None,
        }
    }

    fn session_mut(&mut self) -> Result<&mut PatchSession, PatchError> {
        if self.runtime.is_some() {
            return Err(PatchError::AlreadyFinalized);
        }

        self.session.as_mut().ok_or(PatchError::AlreadyFinalized)
    }
}

static PATCH_MANAGER: OnceLock<Mutex<PatchManager>> = OnceLock::new();

fn patch_manager() -> MutexGuard<'static, PatchManager> {
    PATCH_MANAGER
        .get_or_init(|| Mutex::new(PatchManager::new()))
        .lock()
        .expect("patch manager mutex poisoned")
}

impl Patch {
    /// Creates a patch at `address` so `function` can be run.
    /// `size` determines how many bytes are overwritten for the call and must be at least five.
    ///
    /// # Safety
    /// It is the responsibility of the caller to ensure that the inserted function is compatible with the original ASM.
    /// That means you generally must not split instructions.
    pub unsafe fn patch_call(
        address: usize,
        function: *const (),
        size: usize,
        save_overwritten: bool,
        allow_return: ReturnType,
    ) -> Self {
        assert!(
            size >= NEAR_JUMP_SIZE,
            "A patch call requires at least five bytes"
        );

        let overwritten = slice::from_raw_parts(address as *const u8, size).to_vec();
        let mut trampoline = Vec::new();

        if save_overwritten {
            trampoline.extend_from_slice(&overwritten);
        }

        let needs_alignment = allow_return != ReturnType::Rax;
        if needs_alignment {
            trampoline.extend_from_slice(&SAVE_RAX);
        }
        trampoline.extend_from_slice(&SAVE_REGISTERS);
        if needs_alignment {
            trampoline.extend_from_slice(&ALIGN_STACK);
        }
        if allow_return != ReturnType::Xmm0 {
            trampoline.extend_from_slice(&SAVE_XMM0);
        }

        push_call(&mut trampoline, function);

        if allow_return != ReturnType::Xmm0 {
            trampoline.extend_from_slice(&LOAD_XMM0);
        }
        if needs_alignment {
            trampoline.extend_from_slice(&UNALIGN_STACK);
        }
        trampoline.extend_from_slice(&LOAD_REGISTERS);
        if needs_alignment {
            trampoline.extend_from_slice(&LOAD_RAX);
        }

        let jump_displacement = trampoline.len() + 1;
        trampoline.extend_from_slice(&[NEAR_JUMP, 0, 0, 0, 0]);
        let trampoline_size = trampoline.len();
        let continuation = address
            .checked_add(size)
            .expect("patch continuation address overflowed");

        Self::prepare_detour(
            address,
            overwritten,
            trampoline_size,
            move |trampoline_address| {
                let mut code = trampoline.clone();
                let next_instruction = trampoline_address
                    .checked_add(code.len())
                    .ok_or(PatchError::AddressOverflow)?;
                let displacement = relative_offset(next_instruction, continuation)?;
                code[jump_displacement..jump_displacement + 4]
                    .copy_from_slice(&displacement.to_le_bytes());
                Ok(code)
            },
        )
        .unwrap_or_else(|error| panic!("Unable to prepare call patch at {address:#x}: {error}"))
    }

    /// Replaces `size` bytes at `address` with a near jump to `trampoline`.
    ///
    /// The trampoline is allocated within range of a 32-bit relative jump and
    /// remains executable for the lifetime of the process. The trampoline must
    /// transfer control to the appropriate continuation itself.
    ///
    /// # Safety
    /// The caller must provide valid machine code, overwrite whole instructions,
    /// and ensure every trampoline exit preserves the surrounding function state.
    pub unsafe fn detour(address: usize, size: usize, trampoline: &[u8]) -> Self {
        let trampoline = trampoline.to_vec();
        Self::detour_with(address, size, trampoline.len(), move |_| {
            Ok(trampoline.clone())
        })
    }

    pub(crate) unsafe fn detour_with<F>(
        address: usize,
        size: usize,
        trampoline_size: usize,
        build: F,
    ) -> Self
    where
        F: Fn(usize) -> Result<Vec<u8>, PatchError>,
    {
        assert!(
            size >= NEAR_JUMP_SIZE,
            "A detour requires at least five bytes"
        );
        assert!(trampoline_size > 0, "A detour trampoline cannot be empty");

        let overwritten = slice::from_raw_parts(address as *const u8, size).to_vec();
        Self::prepare_detour(address, overwritten, trampoline_size, build)
            .unwrap_or_else(|error| panic!("Unable to prepare detour at {address:#x}: {error}"))
    }

    unsafe fn prepare_detour<F>(
        address: usize,
        overwritten: Vec<u8>,
        trampoline_size: usize,
        build: F,
    ) -> Result<Self, PatchError>
    where
        F: Fn(usize) -> Result<Vec<u8>, PatchError>,
    {
        let mut manager = patch_manager();
        let session = manager.session_mut()?;
        session.ensure_patch_does_not_overlap(address, overwritten.len())?;

        let trampoline_address = session.allocate_trampoline(address, trampoline_size, &build)?;
        let replacement = build_near_jump(address, trampoline_address, overwritten.len())?;

        session.pending.push(PendingPatch {
            address,
            overwritten: overwritten.clone(),
            replacement,
        });

        Ok(Self {
            address,
            size: overwritten.len(),
            overwritten,
            trampoline: Some(CodeAllocation {
                address: trampoline_address,
            }),
        })
    }

    pub unsafe fn overwrite(address: usize, data: &[u8]) -> Self {
        assert!(!data.is_empty(), "A patch must overwrite at least one byte");

        let overwritten = slice::from_raw_parts(address as *const u8, data.len()).to_vec();
        let mut manager = patch_manager();
        let session = manager
            .session_mut()
            .unwrap_or_else(|error| panic!("Unable to prepare patch at {address:#x}: {error}"));
        session
            .ensure_patch_does_not_overlap(address, data.len())
            .unwrap_or_else(|error| panic!("Unable to prepare patch at {address:#x}: {error}"));
        session.pending.push(PendingPatch {
            address,
            overwritten: overwritten.clone(),
            replacement: data.to_vec(),
        });

        Self {
            address,
            size: data.len(),
            overwritten,
            trampoline: None,
        }
    }

    pub(crate) fn trampoline_address(&self) -> Option<usize> {
        self.trampoline.map(|allocation| allocation.address)
    }
}

impl PatchSession {
    fn ensure_patch_does_not_overlap(&self, address: usize, size: usize) -> Result<(), PatchError> {
        if size == 0 {
            return Err(PatchError::EmptyPatch);
        }

        let end = address
            .checked_add(size)
            .ok_or(PatchError::AddressOverflow)?;
        for patch in &self.pending {
            let patch_end = patch
                .address
                .checked_add(patch.replacement.len())
                .ok_or(PatchError::AddressOverflow)?;
            if address < patch_end && patch.address < end {
                return Err(PatchError::OverlappingPatch {
                    first: patch.address,
                    second: address,
                });
            }
        }

        Ok(())
    }

    fn allocate_trampoline<F>(
        &mut self,
        hook: usize,
        trampoline_size: usize,
        build: &F,
    ) -> Result<usize, PatchError>
    where
        F: Fn(usize) -> Result<Vec<u8>, PatchError>,
    {
        let page_size = MmapOptions::page_size();
        if trampoline_size > page_size {
            return Err(PatchError::TrampolineTooLarge {
                size: trampoline_size,
                capacity: page_size,
            });
        }

        let hook_next_instruction = hook
            .checked_add(NEAR_JUMP_SIZE)
            .ok_or(PatchError::AddressOverflow)?;

        for page in &mut self.pages {
            let Some(range) = slot_range(page.used, trampoline_size, page_size) else {
                continue;
            };
            let trampoline_address = (page.mapping.as_ptr() as usize)
                .checked_add(range.start)
                .ok_or(PatchError::AddressOverflow)?;
            if relative_offset(hook_next_instruction, trampoline_address).is_err() {
                continue;
            }

            let code = match build(trampoline_address) {
                Ok(code) => code,
                Err(PatchError::RelativeJumpOutOfRange { .. }) => continue,
                Err(error) => return Err(error),
            };
            validate_trampoline_size(trampoline_size, code.len())?;
            page.mapping[range.clone()].copy_from_slice(&code);
            page.used = range.end;
            return Ok(trampoline_address);
        }

        self.allocate_trampoline_page(hook, trampoline_size, build)
    }

    fn allocate_trampoline_page<F>(
        &mut self,
        hook: usize,
        trampoline_size: usize,
        build: &F,
    ) -> Result<usize, PatchError>
    where
        F: Fn(usize) -> Result<Vec<u8>, PatchError>,
    {
        let page_size = MmapOptions::page_size();
        let granularity = MmapOptions::allocation_granularity();
        let hook_next_instruction = hook
            .checked_add(NEAR_JUMP_SIZE)
            .ok_or(PatchError::AddressOverflow)?;
        let (minimum, maximum) = candidate_bounds(hook_next_instruction, page_size, granularity)?;
        let mut last_error = None;

        for candidate in CandidateAddresses::new(hook, minimum, maximum, granularity)? {
            if let Some(address) = self.try_allocate_page(
                candidate,
                hook_next_instruction,
                trampoline_size,
                build,
                &mut last_error,
            )? {
                return Ok(address);
            }
        }

        Err(PatchError::NoMemoryCave { hook, last_error })
    }

    fn try_allocate_page<F>(
        &mut self,
        candidate: usize,
        hook_next_instruction: usize,
        trampoline_size: usize,
        build: &F,
        last_error: &mut Option<String>,
    ) -> Result<Option<usize>, PatchError>
    where
        F: Fn(usize) -> Result<Vec<u8>, PatchError>,
    {
        let page_size = MmapOptions::page_size();
        let options =
            MmapOptions::new(page_size).map_err(|error| PatchError::Mapping(error.to_string()))?;
        let mut mapping = match options.with_address(candidate).map_mut() {
            Ok(mapping) => mapping,
            Err(error) => {
                *last_error = Some(error.to_string());
                return Ok(None);
            }
        };

        let actual_address = mapping.as_ptr() as usize;
        if actual_address != candidate {
            *last_error = Some(format!(
                "requested {candidate:#x}, but the mapping was placed at {actual_address:#x}"
            ));
            return Ok(None);
        }
        if relative_offset(hook_next_instruction, actual_address).is_err() {
            *last_error = Some(format!(
                "mapping at {actual_address:#x} was outside rel32 range"
            ));
            return Ok(None);
        }

        let code = match build(actual_address) {
            Ok(code) => code,
            Err(PatchError::RelativeJumpOutOfRange { .. }) => {
                *last_error = Some(format!(
                    "trampoline exits were unreachable from {actual_address:#x}"
                ));
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        validate_trampoline_size(trampoline_size, code.len())?;
        mapping[..trampoline_size].copy_from_slice(&code);
        self.pages.push(MutableCodePage {
            mapping,
            used: trampoline_size,
        });

        Ok(Some(actual_address))
    }
}

fn push_call(code: &mut Vec<u8>, function: *const ()) {
    code.extend_from_slice(&CALL_BYTES);
    code.extend_from_slice(&(function as usize).to_le_bytes());
}

fn validate_trampoline_size(expected: usize, actual: usize) -> Result<(), PatchError> {
    if expected == actual {
        Ok(())
    } else {
        Err(PatchError::UnexpectedTrampolineSize { expected, actual })
    }
}

fn slot_range(used: usize, size: usize, capacity: usize) -> Option<Range<usize>> {
    let start = align_up(used, TRAMPOLINE_ALIGNMENT)?;
    let end = start.checked_add(size)?;
    (end <= capacity).then_some(start..end)
}

fn align_down(value: usize, alignment: usize) -> usize {
    value - value % alignment
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    let remainder = value % alignment;
    if remainder == 0 {
        Some(value)
    } else {
        value.checked_add(alignment - remainder)
    }
}

struct CandidateAddresses {
    hook: usize,
    minimum: usize,
    maximum: usize,
    granularity: usize,
    lower: Option<usize>,
    upper: Option<usize>,
}

impl CandidateAddresses {
    fn new(
        hook: usize,
        minimum: usize,
        maximum: usize,
        granularity: usize,
    ) -> Result<Self, PatchError> {
        let lower = align_down(hook.min(maximum), granularity);
        let lower = (lower >= minimum).then_some(lower);
        let upper = align_up(hook.max(minimum), granularity).ok_or(PatchError::AddressOverflow)?;
        let mut upper = (upper <= maximum).then_some(upper);

        if upper == lower {
            upper = upper
                .and_then(|candidate| candidate.checked_add(granularity))
                .filter(|candidate| *candidate <= maximum);
        }

        Ok(Self {
            hook,
            minimum,
            maximum,
            granularity,
            lower,
            upper,
        })
    }
}

impl Iterator for CandidateAddresses {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let take_lower = match (self.lower, self.upper) {
            (Some(lower), Some(upper)) => lower.abs_diff(self.hook) <= upper.abs_diff(self.hook),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return None,
        };

        if take_lower {
            let candidate = self.lower?;
            self.lower = candidate
                .checked_sub(self.granularity)
                .filter(|next| *next >= self.minimum);
            Some(candidate)
        } else {
            let candidate = self.upper?;
            self.upper = candidate
                .checked_add(self.granularity)
                .filter(|next| *next <= self.maximum);
            Some(candidate)
        }
    }
}

fn candidate_bounds(
    next_instruction: usize,
    page_size: usize,
    granularity: usize,
) -> Result<(usize, usize), PatchError> {
    let minimum = (next_instruction as i128 + i32::MIN as i128).max(0) as usize;
    let maximum_mapping_base = usize::MAX
        .checked_sub(page_size.saturating_sub(1))
        .ok_or(PatchError::AddressOverflow)?;
    let maximum =
        (next_instruction as i128 + i32::MAX as i128).min(maximum_mapping_base as i128) as usize;
    let minimum = align_up(minimum, granularity).ok_or(PatchError::AddressOverflow)?;
    let maximum = align_down(maximum, granularity);

    if minimum > maximum {
        Err(PatchError::NoMemoryCave {
            hook: next_instruction.saturating_sub(NEAR_JUMP_SIZE),
            last_error: None,
        })
    } else {
        Ok((minimum, maximum))
    }
}

fn build_near_jump(source: usize, destination: usize, size: usize) -> Result<Vec<u8>, PatchError> {
    if size < NEAR_JUMP_SIZE {
        return Err(PatchError::UnexpectedTrampolineSize {
            expected: NEAR_JUMP_SIZE,
            actual: size,
        });
    }

    let next_instruction = source
        .checked_add(NEAR_JUMP_SIZE)
        .ok_or(PatchError::AddressOverflow)?;
    let displacement = relative_offset(next_instruction, destination)?;
    let mut patch = Vec::with_capacity(size);
    patch.push(NEAR_JUMP);
    patch.extend_from_slice(&displacement.to_le_bytes());
    patch.resize(size, 0x90);
    Ok(patch)
}

pub(crate) fn relative_offset(
    next_instruction: usize,
    destination: usize,
) -> Result<i32, PatchError> {
    let displacement = destination as i128 - next_instruction as i128;
    i32::try_from(displacement).map_err(|_| PatchError::RelativeJumpOutOfRange {
        next_instruction,
        destination,
    })
}

struct SourceProtection {
    address: usize,
    old: PAGE_PROTECTION_FLAGS,
}

unsafe fn protect_patch_sources(
    patches: &[PendingPatch],
) -> Result<Vec<SourceProtection>, PatchError> {
    let page_size = MmapOptions::page_size();
    let mut source_pages = BTreeSet::new();

    for patch in patches {
        let last_byte = patch
            .address
            .checked_add(patch.replacement.len() - 1)
            .ok_or(PatchError::AddressOverflow)?;
        let first_page = align_down(patch.address, page_size);
        let last_page = align_down(last_byte, page_size);
        let mut page = first_page;

        loop {
            source_pages.insert(page);
            if page == last_page {
                break;
            }
            page = page
                .checked_add(page_size)
                .ok_or(PatchError::AddressOverflow)?;
        }
    }

    let mut protections = Vec::with_capacity(source_pages.len());
    for address in source_pages {
        let mut old = PAGE_PROTECTION_FLAGS(0);
        if let Err(error) = VirtualProtect(
            address as *const c_void,
            page_size,
            PAGE_EXECUTE_READWRITE,
            &mut old,
        ) {
            restore_patch_sources(&protections)?;
            return Err(PatchError::Protection {
                address,
                error: error.to_string(),
            });
        }
        protections.push(SourceProtection { address, old });
    }

    Ok(protections)
}

unsafe fn restore_patch_sources(protections: &[SourceProtection]) -> Result<(), PatchError> {
    let page_size = MmapOptions::page_size();
    let mut first_error = None;

    for protection in protections.iter().rev() {
        let mut ignored = PAGE_PROTECTION_FLAGS(0);
        if let Err(error) = VirtualProtect(
            protection.address as *const c_void,
            page_size,
            protection.old,
            &mut ignored,
        ) {
            first_error.get_or_insert_with(|| PatchError::Protection {
                address: protection.address,
                error: error.to_string(),
            });
        }
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

unsafe fn validate_patch_sources(patches: &[PendingPatch]) -> Result<(), PatchError> {
    for patch in patches {
        let current = slice::from_raw_parts(patch.address as *const u8, patch.overwritten.len());
        if current != patch.overwritten {
            return Err(PatchError::SourceChanged {
                address: patch.address,
            });
        }
    }

    Ok(())
}

/// Seals all prepared trampoline pages as executable and installs every prepared patch.
///
/// # Safety
/// No other thread may execute a patch source while its instructions are being replaced.
pub(crate) unsafe fn finalize_patches() -> Result<(), PatchError> {
    let mut manager = patch_manager();
    if manager.runtime.is_some() || manager.session.is_none() {
        return Err(PatchError::AlreadyFinalized);
    }

    let session = manager.session.take().ok_or(PatchError::AlreadyFinalized)?;
    let mut executable_pages = Vec::with_capacity(session.pages.len());
    for page in session.pages {
        match page.mapping.make_exec() {
            Ok(mapping) => executable_pages.push(mapping),
            Err((_mapping, error)) => return Err(PatchError::Mapping(error.to_string())),
        }
    }

    let process = GetCurrentProcess();
    for page in &executable_pages {
        FlushInstructionCache(process, Some(page.as_ptr() as *const c_void), page.len()).map_err(
            |error| PatchError::InstructionCache {
                address: page.as_ptr() as usize,
                error: error.to_string(),
            },
        )?;
    }

    validate_patch_sources(&session.pending)?;
    let protections = protect_patch_sources(&session.pending)?;
    if let Err(error) = validate_patch_sources(&session.pending) {
        restore_patch_sources(&protections)?;
        return Err(error);
    }

    let runtime = PatchRuntime {
        pages: executable_pages,
        patches: session.pending,
    };
    let page_count = runtime.pages.len();
    let patch_count = runtime.patches.len();
    manager.runtime = Some(runtime);

    let runtime = manager
        .runtime
        .as_ref()
        .expect("patch runtime disappeared during installation");
    for patch in &runtime.patches {
        std::ptr::copy_nonoverlapping(
            patch.replacement.as_ptr(),
            patch.address as *mut u8,
            patch.replacement.len(),
        );
    }

    for patch in &runtime.patches {
        FlushInstructionCache(
            process,
            Some(patch.address as *const c_void),
            patch.replacement.len(),
        )
        .expect("unable to flush the instruction cache after installing patches");
    }
    restore_patch_sources(&protections)
        .expect("unable to restore source page protections after installing patches");

    log::info!("Installed {patch_count} patch(es) using {page_count} shared trampoline page(s)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Memory::{
        VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
    };

    const DEAD_BEEF: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];

    fn dummy() {
        println!("Dummy function");
    }

    #[test]
    fn rel32_boundaries_are_checked() {
        let next_instruction = 0x2_0000_0000usize;

        assert_eq!(
            relative_offset(next_instruction, next_instruction - 0x8000_0000).unwrap(),
            i32::MIN
        );
        assert_eq!(
            relative_offset(next_instruction, next_instruction + 0x7fff_ffff).unwrap(),
            i32::MAX
        );
        assert!(relative_offset(next_instruction, next_instruction - 0x8000_0001).is_err());
        assert!(relative_offset(next_instruction, next_instruction + 0x8000_0000).is_err());
    }

    #[test]
    fn slots_are_aligned_and_do_not_overlap() {
        let first = slot_range(0, 135, 4096).unwrap();
        let second = slot_range(first.end, 129, 4096).unwrap();

        assert_eq!(first, 0..135);
        assert_eq!(second, 144..273);
        assert!(first.end <= second.start);
        assert!(slot_range(4090, 16, 4096).is_none());
    }

    #[test]
    fn mapping_candidates_are_ordered_by_distance_from_hook() {
        let candidates = CandidateAddresses::new(0x1f000, 0, 0x40000, 0x10000)
            .unwrap()
            .collect::<Vec<_>>();

        assert_eq!(candidates, [0x20000, 0x10000, 0x30000, 0, 0x40000]);
    }

    #[test]
    fn patch_calls_share_a_page_and_install_together() {
        unsafe {
            let size = 64;
            let test_memory =
                VirtualAlloc(None, size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE);
            assert!(!test_memory.is_null(), "Failed to allocate test memory");

            let test_data = DEAD_BEEF.to_vec().repeat(size / DEAD_BEEF.len());
            std::ptr::copy_nonoverlapping(test_data.as_ptr(), test_memory as *mut u8, size);

            let first_address = test_memory as usize;
            let second_address = first_address + 32;
            let first = Patch::patch_call(
                first_address,
                dummy as *const (),
                10,
                true,
                ReturnType::None,
            );
            let second = Patch::patch_call(
                second_address,
                dummy as *const (),
                10,
                true,
                ReturnType::None,
            );

            let first_trampoline = first.trampoline.expect("first patch has no trampoline");
            let second_trampoline = second.trampoline.expect("second patch has no trampoline");
            assert_eq!(
                align_down(first_trampoline.address, MmapOptions::page_size()),
                align_down(second_trampoline.address, MmapOptions::page_size())
            );
            assert_ne!(first_trampoline.address, second_trampoline.address);

            let replayed = slice::from_raw_parts(first_trampoline.address as *const u8, 10);
            assert_eq!(&replayed[..DEAD_BEEF.len()], &DEAD_BEEF);
            assert_eq!(
                slice::from_raw_parts(first_address as *const u8, 4),
                &DEAD_BEEF
            );

            finalize_patches().unwrap();

            assert_eq!(*(first_address as *const u8), NEAR_JUMP);
            assert_eq!(*(second_address as *const u8), NEAR_JUMP);
            VirtualFree(test_memory, 0, MEM_RELEASE).unwrap();
        }
    }
}
