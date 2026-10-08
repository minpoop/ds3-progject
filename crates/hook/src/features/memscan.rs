//! Reading this process's own memory without ever faulting: `ReadProcessMemory` on ourselves returns an error for an
//! unreadable page instead of raising an access violation.
use core::ffi::c_void;
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS, PAGE_NOCACHE, PAGE_WRITECOMBINE};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub base: usize,
    pub size: usize,
    #[allow(dead_code)]
    pub protect: u32,
    #[allow(dead_code)]
    pub kind: u32,
}

/// Committed, readable regions of the address space (regions bigger than `max_region` are skipped).
pub fn regions(max_region: usize) -> Vec<Region> {
    let mut out = Vec::new();
    let mut addr: usize = 0;
    unsafe {
        loop {
            let mut mbi: MEMORY_BASIC_INFORMATION = core::mem::zeroed();
            if VirtualQuery(addr as *const c_void, &mut mbi, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
                break;
            }
            let base = mbi.BaseAddress as usize;
            let size = mbi.RegionSize;
            if mbi.State == MEM_COMMIT && mbi.Protect != 0 && mbi.Protect & (PAGE_GUARD | PAGE_NOACCESS | PAGE_NOCACHE | PAGE_WRITECOMBINE) == 0 && size <= max_region {
                out.push(Region { base, size, protect: mbi.Protect, kind: mbi.Type });
            }
            match base.checked_add(size) {
                Some(next) if next > addr && next < 0x7FFF_FFFF_0000 => addr = next,
                _ => break,
            }
        }
    }
    out
}

/// Copy up to `buf.len()` bytes from `addr`; returns how many were copied (0 if unreadable).
pub fn read_into(addr: usize, buf: &mut [u8]) -> usize {
    let mut got: usize = 0;
    let ok = unsafe { ReadProcessMemory(GetCurrentProcess(), addr as *const c_void, buf.as_mut_ptr() as *mut c_void, buf.len(), &mut got) };
    if ok == 0 && got == 0 {
        0
    } else {
        got
    }
}

pub fn read(addr: usize, len: usize) -> Option<Vec<u8>> {
    let mut v = vec![0u8; len];
    let n = read_into(addr, &mut v);
    if n == len {
        Some(v)
    } else {
        None
    }
}

/// Are `len` bytes at `addr` readable right now? (Checks both ends; for a pointer chain that must not fault.)
pub fn readable(addr: usize, len: usize) -> bool {
    if addr < 0x10000 || len == 0 {
        return false;
    }
    let mut b = [0u8; 8];
    read_into(addr, &mut b) == 8 && read_into(addr + len.saturating_sub(8), &mut b) == 8
}
