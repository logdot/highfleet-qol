//! Simple patching library for windows processes.

use core::slice;
use std::ffi::c_void;

use mmap_rs::{MemoryAreas, Mmap, MmapOptions};
use windows::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS};

#[derive(PartialEq, Eq)]
pub enum ReturnType {
    None,
    Rax,
    Xmm0,
}

const CALL_BYTES: [u8; 8] = [0xff, 0x15, 0x02, 0x00, 0x00, 0x00, 0xeb, 0x08];
const NEAR_JUMP: [u8; 1] = [0xe9];

// PUSH RAX
const SAVE_RAX: [u8; 1] = [0x50];
// MOVDQU [RSP + 0x00], XMM0
const SAVE_XMM0: [u8; 5] = [0xF3, 0x0F, 0x7F, 0x04, 0x24];
const SAVE_REGISTERS: [u8; 44] = [
    // PUSH RCX
    0x51,
    // PUSH RDX
    0x52,
    // PUSH R8
    0x41, 0x50,
    // PUSH R9
    0x41, 0x51,
    // PUSH R10
    0x41, 0x52,
    // PUSH R11
    0x41, 0x53,
    // SUB RSP, 0x60
    0x48, 0x83, 0xEC, 0x60,
    // MOVDQU [RSP + 0x10], XMM1
    0xF3, 0x0F, 0x7F, 0x4C, 0x24, 0x10,
    // MOVDQU [RSP + 0x20], XMM2
    0xF3, 0x0F, 0x7F, 0x54, 0x24, 0x20,
    // MOVDQU [RSP + 0x30], XMM3
    0xF3, 0x0F, 0x7F, 0x5C, 0x24, 0x30,
    // MOVDQU [RSP + 0x40], XMM4
    0xF3, 0x0F, 0x7F, 0x64, 0x24, 0x40,
    // MOVDQU [RSP + 0x50], XMM5
    0xF3, 0x0F, 0x7F, 0x6C, 0x24, 0x50
];

// POP RAX
const LOAD_RAX: [u8; 1] = [0x58];
// MOVDQU XMM0, [RSP + 0x00]
const LOAD_XMM0: [u8; 5] = [0xF3, 0x0F, 0x6F, 0x04, 0x24];
const LOAD_REGISTERS: [u8; 44] = [
    // MOVDQU XMM1, [RSP + 0x10]
    0xF3, 0x0F, 0x6F, 0x4C, 0x24, 0x10,
    // MOVDQU XMM2, [RSP + 0x20]
    0xF3, 0x0F, 0x6F, 0x54, 0x24, 0x20,
    // MOVDQU XMM3, [RSP + 0x30]
    0xF3, 0x0F, 0x6F, 0x5C, 0x24, 0x30,
    // MOVDQU XMM4, [RSP + 0x40]
    0xF3, 0x0F, 0x6F, 0x64, 0x24, 0x40,
    // MOVDQU XMM5, [RSP + 0x50]
    0xF3, 0x0F, 0x6F, 0x6C, 0x24, 0x50,
    // ADD RSP, 0x60
    0x48, 0x83, 0xC4, 0x60,
    // POP R11
    0x41, 0x5B,
    // POP R10
    0x41, 0x5A,
    // POP R9
    0x41, 0x59,
    // POP R8
    0x41, 0x58,
    // POP RDX
    0x5A,
    // POP RCX
    0x59
];

/// A struct representing a single patch done to the game's code.
/// A patch can be undone by calling `unpatch`.
pub struct Patch {
    size: usize,
    overwritten: Vec<u8>,
    mmap: Option<Mmap>,
}

impl Patch {
    /// Creates a patch at `address` so `function` can be run.
    /// `size` determines how many bytes are overwritten for call, must be at least 4.
    ///
    /// # Safety
    /// It is the responsibility of the caller to ensure that the inserted function is compatible with the original code.
    pub unsafe fn patch_call(address: usize, function: *const (), size: usize, save_overwritten: bool, allow_return: ReturnType) -> Self {
        // Set EXECUTE READWRITE to allow writing to code section
        let mut old_protect = PAGE_PROTECTION_FLAGS(0);

        VirtualProtect(
            address as *mut c_void,
            0x100,
            PAGE_EXECUTE_READWRITE,
            &mut old_protect as *mut _,
        ).unwrap();

        // Save the overwritten bytes
        let process_bytes = slice::from_raw_parts(address as *const u8, size);
        let overwritten = process_bytes.to_vec();

        let memory_cave = search_memory_cave(address).expect("No memory cave found");

        let mut mmap = MmapOptions::new(MmapOptions::page_size()).unwrap()
            .with_address(memory_cave)
            .map_mut().expect("Unable to allocate memory map");

        let address = address as *mut u8;

        let mem = mmap.as_mut_ptr();

        // Write relative jump
        std::ptr::copy_nonoverlapping(NEAR_JUMP.as_ptr(), address, NEAR_JUMP.len());
        let jump_offset = mem as isize - address as isize - 5;
        let jump_offset: i32 = jump_offset.try_into().expect("Jump offset greater than 32 bits");
        std::ptr::copy_nonoverlapping(&jump_offset as *const _ as *const u8, address.add(1), 4);

        // Write nop slide
        let nops = vec![0x90; size - 5];
        write_data(address, &mut 5, &nops);

        // Restore old protection on code section
        VirtualProtect(
            address as *mut c_void,
            0x100,
            old_protect,
            &mut old_protect as *mut _,
        ).unwrap();

        // Keeps track of offset in memory
        let mut offset = 0;

        if save_overwritten {
            // Write overwritten bytes to memory
            write_data(mem, &mut offset, &overwritten);
        }

        // If we aren't returning in RAX, we need to save it to not mess up the original code
        if allow_return != ReturnType::Rax {
            write_data(mem, &mut offset, &SAVE_RAX);
        }
        // Save clobbered registers to save their state
        write_data(mem, &mut offset, &SAVE_REGISTERS);
        // if we aren't returning in XMM0, save it to not mess up the original code
        if allow_return != ReturnType::Xmm0 {
            write_data(mem, &mut offset, &SAVE_XMM0);
        }

        // Write the call to the memory
        write_call(mem, &mut offset, function);

        // If we aren't returning in XMM0, load it to it's original state
        if allow_return != ReturnType::Xmm0 {
            write_data(mem, &mut offset, &LOAD_XMM0);
        }
        // Load clobbered registers to restore state
        write_data(mem, &mut offset, &LOAD_REGISTERS);
        // If we aren't returning in RAX, load it to it's original state
        if allow_return != ReturnType::Rax {
            write_data(mem, &mut offset, &LOAD_RAX);
        }

        // Jump back to the original code
        std::ptr::copy_nonoverlapping(NEAR_JUMP.as_ptr(), mem.add(offset), NEAR_JUMP.len());
        let jump_offset = address.add(size) as isize - mem.add(offset) as isize - 5;
        let jump_offset: i32 = jump_offset.try_into().expect("Jump offset greater than 32 bits");
        offset += NEAR_JUMP.len();
        std::ptr::copy_nonoverlapping(&jump_offset as *const _ as *const u8, mem.add(offset), 4);

        let mmap = mmap.make_exec().unwrap();

        Patch {
            size,
            overwritten,
            mmap: Some(mmap),
        }
    }

    pub unsafe fn overwrite(address: usize, data: &[u8]) -> Self {
        // Set EXECUTE READWRITE to allow writing to code section
        let mut old_protect = PAGE_PROTECTION_FLAGS(0);

        VirtualProtect(
            address as *mut c_void,
            0x100,
            PAGE_EXECUTE_READWRITE,
            &mut old_protect as *mut _,
        ).unwrap();

        // Save the overwritten bytes
        let process_bytes = slice::from_raw_parts(address as *const u8, data.len());
        let overwritten = process_bytes.to_vec();

        let address = address as *mut u8;

        std::ptr::copy_nonoverlapping(data.as_ptr(), address, data.len());

        // Restore old protection on code section
        VirtualProtect(
            address as *mut c_void,
            0x100,
            old_protect,
            &mut old_protect as *mut _,
        ).unwrap();

        Patch {
            size: data.len(),
            overwritten,
            mmap: None,
        }
    }
}

unsafe fn write_call(address: *mut u8, offset: &mut usize, function: *const ()) {
    let function_ptr = &function as *const _ as *const u8;

    std::ptr::copy_nonoverlapping(CALL_BYTES.as_ptr(), address.add(*offset), CALL_BYTES.len());
    *offset += CALL_BYTES.len();

    std::ptr::copy_nonoverlapping(function_ptr, address.add(*offset), 8);
    // Pointers to functions in x64 are always 8 bytes
    *offset += 8;
}

unsafe fn write_data(address: *mut u8, offset: &mut usize, data: &[u8]) {
    std::ptr::copy_nonoverlapping(data.as_ptr(), address.add(*offset), data.len());
    *offset += data.len();
}

/// Searches for a valid memory region that can be used for code within the 4GB address space for a jump.
fn search_memory_cave(address: usize) -> Option<usize> {
    // 0x80000000 is equivalent to 2GB
    let lower_bound = address-0x80000000 + MmapOptions::allocation_granularity();
    let upper_bound = address+0x80000000 - MmapOptions::allocation_granularity();

    (lower_bound..upper_bound).step_by(MmapOptions::allocation_granularity()).find(|address| {
        MemoryAreas::query(*address).unwrap().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEAD_BEEF: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];

    fn dummy() {
        println!("Dummy function");
    }

    #[test]
    fn test_patch_call() {
        let address_space = DEAD_BEEF.to_vec().repeat(10);

        let address = address_space.as_ptr() as usize;
        let size = 10;

        let patch = unsafe { Patch::patch_call(address, dummy as *const (), size, true, ReturnType::None) };

        // Check that bytes successfully written into mmap
        let mmap = patch.mmap.unwrap().as_ptr();
        let overwritten = unsafe { slice::from_raw_parts(mmap, size) };
        assert_eq!(*overwritten, DEAD_BEEF.to_vec().repeat(10)[..size])
    }
}
