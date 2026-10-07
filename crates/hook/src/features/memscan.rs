//! Reading this process's own memory without ever faulting: `ReadProcessMemory` on ourselves returns an error for an
//! unreadable page instead of raising an access violation.
use core::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
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

/// Copy `bytes` to `addr` in this process. `WriteProcessMemory` on ourselves fails with an error (instead of an access
/// violation) when the page cannot be written, so a wrong address is harmless. Returns whether everything was written.
pub fn write_into(addr: usize, bytes: &[u8]) -> bool {
    let mut put: usize = 0;
    let ok = unsafe { WriteProcessMemory(GetCurrentProcess(), addr as *mut c_void, bytes.as_ptr() as *const c_void, bytes.len(), &mut put) };
    ok != 0 && put == bytes.len()
}

/// Addresses where `text` sits in this process as a whole UTF-16 string: NUL-terminated, and not the end of a longer
/// string (the two bytes before it are not a plain letter).
pub fn find_exact_utf16(text: &str, budget: Duration) -> Vec<usize> {
    const CHUNK: usize = 8 << 20;
    let needle: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let finder = memchr::memmem::Finder::new(&needle);
    let started = Instant::now();
    let mut hits = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    let overlap = needle.len() + 8;
    'regions: for r in regions(1 << 30) {
        let mut off = 0usize;
        while off < r.size {
            if started.elapsed() > budget {
                break 'regions;
            }
            let want = CHUNK.min(r.size - off);
            let got = read_into(r.base + off, &mut buf[..want]);
            for p in finder.find_iter(&buf[..got]) {
                let addr = r.base + off + p;
                if addr % 2 != 0 || p + needle.len() + 2 > got || buf[p + needle.len()..p + needle.len() + 2] != [0, 0] {
                    continue;
                }
                // a letter (printable ASCII followed by a zero byte) right before it means this is the tail of a longer word
                if p >= 2 && buf[p - 1] == 0 && (0x20..=0x7E).contains(&buf[p - 2]) {
                    continue;
                }
                hits.push(addr);
            }
            if off + want >= r.size {
                break;
            }
            off += (want - overlap).max(1);
        }
    }
    hits.sort_unstable();
    hits.dedup();
    hits
}


/// Every 8-byte-aligned place in memory that holds one of `targets` as a little-endian 64-bit value (a pointer to it).
/// Returns (where it was found, which target). Stops early when the time budget is used up.
pub fn find_pointers(targets: &[usize], budget: Duration) -> Vec<(usize, usize)> {
    const CHUNK: usize = 8 << 20;
    let finders: Vec<(usize, memchr::memmem::Finder<'static>)> = targets.iter().map(|&t| (t, memchr::memmem::Finder::new(&(t as u64).to_le_bytes()).into_owned())).collect();
    let started = Instant::now();
    let mut hits = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    'regions: for r in regions(1 << 30) {
        let mut off = 0usize;
        while off < r.size {
            if started.elapsed() > budget {
                break 'regions;
            }
            let want = CHUNK.min(r.size - off);
            let got = read_into(r.base + off, &mut buf[..want]);
            for (target, f) in &finders {
                for p in f.find_iter(&buf[..got]) {
                    let addr = r.base + off + p;
                    if addr % 8 == 0 {
                        hits.push((addr, *target));
                    }
                }
            }
            if off + want >= r.size {
                break;
            }
            off += (want - 16).max(1);
        }
    }
    hits.sort_unstable();
    hits.dedup();
    hits
}
