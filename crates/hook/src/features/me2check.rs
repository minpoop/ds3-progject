//! A read-only look at what ModEngine2 left in the running game (private test kits, `--probe`; never writes, never calls a game
//! function). ModEngine2 serves loose files from the mod folder by hooking a function of the game's program file at a fixed
//! address (`virtual_to_archive_path`, offset 0x7D660 from the start of the program in its source for Dark Souls III 1.15.2)
//! and by patching a few places. If the program file of the player is not the one it was made for, those hooks and patches
//! are somewhere else, or nowhere - and then no file of the mod folder would ever be loaded in place of an archived one,
//! whatever is put there. The kit 0.8 log showed ModEngine2 asking about only four files of the mod folder, none of them a
//! model; this says whether its hook is in the place its source says, and where it jumps to.
//!
//! The report is `logs/me2-hook.txt`: the program file's range, the first bytes at the two addresses ModEngine2's source
//! names (and where a jump there leads, by module name), and how many times the signatures of ModEngine2's loose-parameter
//! patch occur in the program file in their original and in their patched form.
use super::memscan;
use ashen_common::{config::HookConfig, logging::Logger, VERSION};
use std::path::Path;
use std::time::Duration;
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GetModuleHandleW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};

/// Where ModEngine2's source hooks `virtual_to_archive_path` in the Dark Souls III program file (offset from its start).
const HOOK_RVA: usize = 0x7D660;
/// Where its source overwrites five bytes with NOPs.
const NOP_PATCH_RVA: usize = 0xEA1B83;
/// The two signatures of the loose-parameter patch: as the game has them, and as ModEngine2 changes them.
const PATTERNS: [(&str, &[u8]); 4] = [
    ("loose-params signature 1, as the game has it", &[0x74, 0x68, 0x48, 0x8B, 0xCF, 0x48, 0x89, 0x5C, 0x24, 0x30, 0xE8]),
    ("loose-params signature 1, as ModEngine2 patches it", &[0xEB, 0x68, 0x48, 0x8B, 0xCF, 0x48, 0x89, 0x5C, 0x24, 0x30, 0xE8]),
    ("loose-params signature 2, as the game has it", &[0x0F, 0x85, 0xC5, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x4C, 0x24, 0x28]),
    ("loose-params signature 2, as ModEngine2 patches it", &[0x0F, 0x84, 0xC5, 0x00, 0x00, 0x00, 0x48, 0x8D, 0x4C, 0x24, 0x28]),
];
const CHUNK: usize = 8 << 20;
/// How much of a piece is read again with the next one (longer than every pattern).
const OVERLAP: usize = 32;

type Reader<'a> = &'a dyn Fn(usize, usize) -> Option<Vec<u8>>;

/// Where the instruction at `addr` jumps to, if it is a jump of the kinds hooking libraries write: `E9 rel32`, `EB rel8`,
/// `FF 25 rel32` (through a pointer; MinHook's relay jumps with an absolute address right after `FF 25 00 00 00 00`), and
/// `mov rax, imm64` / `mov r11, imm64` followed by a jump through that register.
pub fn jump_target(read: Reader, addr: usize) -> Option<usize> {
    let b = read(addr, 16)?;
    match b.as_slice() {
        [0xE9, r @ ..] if r.len() >= 4 => Some((addr as i64 + 5 + i32::from_le_bytes([r[0], r[1], r[2], r[3]]) as i64) as usize),
        [0xEB, r, ..] => Some((addr as i64 + 2 + *r as i8 as i64) as usize),
        [0xFF, 0x25, r @ ..] if r.len() >= 4 => {
            let ptr = (addr as i64 + 6 + i32::from_le_bytes([r[0], r[1], r[2], r[3]]) as i64) as usize;
            let target = read(ptr, 8)?;
            Some(u64::from_le_bytes(target.try_into().ok()?) as usize)
        }
        [0x48, 0xB8, r @ ..] if r.len() >= 10 && r[8] == 0xFF && r[9] == 0xE0 => Some(u64::from_le_bytes(r[..8].try_into().ok()?) as usize),
        [0x49, 0xBB, r @ ..] if r.len() >= 11 && r[8] == 0x41 && r[9] == 0xFF && r[10] == 0xE3 => Some(u64::from_le_bytes(r[..8].try_into().ok()?) as usize),
        _ => None,
    }
}

/// The chain of jumps from `addr` (at most four hops): every address it leads to, in order.
pub fn jump_chain(read: Reader, addr: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    let mut at = addr;
    for _ in 0..4 {
        match jump_target(read, at) {
            Some(next) if next != at && !chain.contains(&next) => {
                chain.push(next);
                at = next;
            }
            _ => break,
        }
    }
    chain
}

/// `SizeOfImage` of a PE file's headers (the start of the module), if they are one.
pub fn size_of_image(headers: &[u8]) -> Option<usize> {
    if headers.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(headers.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if headers.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    // the optional header starts 24 bytes after "PE"; SizeOfImage is 56 bytes into it (PE32+ and PE32 alike)
    Some(u32::from_le_bytes(headers.get(pe + 24 + 56..pe + 24 + 60)?.try_into().ok()?) as usize)
}

/// How often each pattern occurs in `data` (a piece of a module), counting only the ones that start before `starts_before`: the
/// next piece starts there and has the rest, so a pattern between two pieces is counted once.
pub fn count_patterns(data: &[u8], starts_before: usize, counts: &mut [usize; PATTERNS.len()]) {
    for (count, (_, pattern)) in counts.iter_mut().zip(PATTERNS) {
        *count += memchr::memmem::find_iter(data, pattern).take_while(|at| *at < starts_before).count();
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// The file name of the module an address lies in.
fn module_of(addr: usize) -> Option<String> {
    let mut module = core::ptr::null_mut();
    let ok = unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, addr as *const u16, &mut module) };
    if ok == 0 || module.is_null() {
        return None;
    }
    let mut buf = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let path = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    path.rsplit(['\\', '/']).next().filter(|s| !s.is_empty()).map(str::to_string)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn describe_chain(log: &Logger, read: Reader, what: &str, addr: usize) {
    match read(addr, 16) {
        None => log.log(&format!("{what} at 0x{addr:X}: cannot be read")),
        Some(bytes) => {
            let chain = jump_chain(read, addr);
            if chain.is_empty() {
                log.log(&format!("{what} at 0x{addr:X}: {} - not a jump: nothing is hooked here (it looks the way the game has it)", hex(&bytes)));
            } else {
                let places: Vec<String> = chain.iter().map(|a| format!("0x{a:X} in {}", module_of(*a).unwrap_or_else(|| "(no module)".to_string()))).collect();
                log.log(&format!("{what} at 0x{addr:X}: {} - a JUMP to {}", hex(&bytes), places.join(", then ")));
            }
        }
    }
}

/// One look. `label` says when (for the log).
fn check(log: &Logger, label: &str) {
    log.log(&format!("--- {label} ---"));
    let me2 = unsafe { GetModuleHandleW(wide("modengine2.dll").as_ptr()) };
    log.log(&if me2.is_null() { "modengine2.dll is not loaded in this process under that name".to_string() } else { format!("modengine2.dll is loaded at 0x{:X}", me2 as usize) });
    let base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
    if base == 0 {
        log.log("the program file's base address is unknown");
        return;
    }
    let read = |addr: usize, len: usize| memscan::read(addr, len);
    let size = read(base, 0x400).and_then(|h| size_of_image(&h));
    log.log(&format!("the program file is at 0x{base:X}, {} bytes in memory", size.map_or("an unknown number of".to_string(), |s| s.to_string())));
    describe_chain(log, &read, &format!("ModEngine2's file hook (offset 0x{HOOK_RVA:X})"), base + HOOK_RVA);
    match read(base + NOP_PATCH_RVA, 8) {
        None => log.log(&format!("offset 0x{NOP_PATCH_RVA:X}: cannot be read")),
        Some(b) => log.log(&format!(
            "offset 0x{NOP_PATCH_RVA:X}: {} - {}",
            hex(&b),
            if b[..5] == [0x90; 5] { "five NOPs: ModEngine2's patch is in place (or the game has them there)" } else { "not NOPs: ModEngine2's patch is not there" }
        )),
    }
    if let Some(size) = size {
        let mut counts = [0usize; PATTERNS.len()];
        let mut buf = vec![0u8; CHUNK];
        let mut off = 0usize;
        let mut unreadable = 0usize;
        while off < size {
            let want = CHUNK.min(size - off);
            let got = memscan::read_into(base + off, &mut buf[..want]);
            let last = off + want >= size;
            if got == 0 {
                unreadable += want;
            } else {
                // a piece that is cut short (unreadable pages) is counted in full up to where it ends
                let limit = if last || got < want { got } else { got.saturating_sub(OVERLAP) };
                count_patterns(&buf[..got], limit, &mut counts);
            }
            if last {
                break;
            }
            off += want.saturating_sub(OVERLAP).max(1);
        }
        for ((what, _), n) in PATTERNS.iter().zip(counts) {
            log.log(&format!("{what}: found {n} time(s) in the program file"));
        }
        if unreadable > 0 {
            log.log(&format!("({unreadable} bytes of the program file could not be read, so those counts may be too low)"));
        }
    }
}

/// Runs on its own thread: one look soon after the start and one when the game is well into its loading.
pub fn thread_body(cfg: HookConfig) {
    let logs = Path::new(&cfg.log_file).parent().map(Path::to_path_buf).unwrap_or_else(|| std::path::PathBuf::from("."));
    let log = Logger::open(&logs.join("me2-hook.txt"), "me2");
    log.log(&format!("me2-hook v{VERSION}: a read-only look at what ModEngine2 left in the game (nothing is written, no game function is called)"));
    for (wait, label) in [(5u64, "5 seconds after the start"), (40, "45 seconds after the start")] {
        std::thread::sleep(Duration::from_secs(wait));
        check(&log, label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn memory(parts: &[(usize, Vec<u8>)]) -> impl Fn(usize, usize) -> Option<Vec<u8>> {
        let map: HashMap<usize, Vec<u8>> = parts.iter().cloned().collect();
        move |addr, len| {
            // a read may start inside a part
            map.iter().find_map(|(start, bytes)| {
                (addr >= *start && addr + len <= start + bytes.len()).then(|| bytes[addr - start..addr - start + len].to_vec()).or_else(|| (addr >= *start && addr < start + bytes.len()).then(|| bytes[addr - start..].iter().copied().chain(std::iter::repeat(0)).take(len).collect()))
            })
        }
    }

    #[test]
    fn the_jumps_hooking_libraries_write_are_followed() {
        // E9 rel32 to a relay that jumps through an absolute address (MinHook's way), landing at 0x7000
        let mut relay = vec![0xFF, 0x25, 0, 0, 0, 0];
        relay.extend(0x7000u64.to_le_bytes());
        let mut hook = vec![0xE9];
        hook.extend((0x5000i32 - (0x1000 + 5)).to_le_bytes());
        hook.extend([0x90; 11]);
        let mem = memory(&[(0x1000, hook), (0x5000, relay), (0x7000, vec![0x48, 0x89, 0x5C, 0x24, 0x08, 0x57, 0x48, 0x83, 0xEC, 0x20, 0, 0, 0, 0, 0, 0])]);
        assert_eq!(jump_chain(&mem, 0x1000), vec![0x5000, 0x7000]);
        // a function as the game has it is not a jump
        assert_eq!(jump_chain(&mem, 0x7000), Vec::<usize>::new());
        // short jump, mov rax / jmp rax, mov r11 / jmp r11
        let mut mov_rax = vec![0x48, 0xB8];
        mov_rax.extend(0xDEAD_0000u64.to_le_bytes());
        mov_rax.extend([0xFF, 0xE0, 0, 0, 0, 0]);
        let mut mov_r11 = vec![0x49, 0xBB];
        mov_r11.extend(0xBEEF_0000u64.to_le_bytes());
        mov_r11.extend([0x41, 0xFF, 0xE3, 0, 0]);
        let mut short = vec![0xEB, 0xFE_u8];
        short.extend([0u8; 14]);
        let mut back = vec![0xEB, 0x10];
        back.extend([0u8; 14]);
        let mem = memory(&[(0x100, mov_rax), (0x200, mov_r11), (0x300, short), (0x400, back)]);
        assert_eq!(jump_target(&mem, 0x100), Some(0xDEAD_0000));
        assert_eq!(jump_target(&mem, 0x200), Some(0xBEEF_0000));
        assert_eq!(jump_target(&mem, 0x300), Some(0x300), "a jump to itself is a loop, not a hook");
        assert_eq!(jump_chain(&mem, 0x300), Vec::<usize>::new());
        assert_eq!(jump_target(&mem, 0x400), Some(0x412));
        // unreadable memory
        assert_eq!(jump_target(&mem, 0x9000), None);
    }

    #[test]
    fn the_size_of_a_module_comes_from_its_headers() {
        let mut h = vec![0u8; 0x400];
        h[..2].copy_from_slice(b"MZ");
        h[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        h[0x80..0x84].copy_from_slice(b"PE\0\0");
        h[0x80 + 24 + 56..0x80 + 24 + 60].copy_from_slice(&0x0510_0000u32.to_le_bytes());
        assert_eq!(size_of_image(&h), Some(0x0510_0000));
        assert_eq!(size_of_image(&h[..0x40]), None);
        h[0] = b'X';
        assert_eq!(size_of_image(&h), None);
        assert_eq!(size_of_image(&[]), None);
    }

    #[test]
    fn the_signatures_are_counted_in_their_two_forms() {
        let mut data = vec![0u8; 200];
        data[10..21].copy_from_slice(PATTERNS[0].1);
        data[50..61].copy_from_slice(PATTERNS[1].1);
        data[90..101].copy_from_slice(PATTERNS[1].1);
        data[150..161].copy_from_slice(PATTERNS[3].1);
        let mut counts = [0usize; 4];
        count_patterns(&data, data.len(), &mut counts);
        assert_eq!(counts, [1, 2, 0, 1]);
        // two pieces that overlap: a pattern in the overlap is counted by the piece it starts in only
        let mut a = [0usize; 4];
        let mut b = [0usize; 4];
        count_patterns(&data[..100], 100 - 32, &mut a);
        count_patterns(&data[100 - 32..], data.len() - (100 - 32), &mut b);
        assert_eq!(a.iter().zip(&b).map(|(x, y)| x + y).collect::<Vec<_>>(), vec![1, 2, 0, 1]);
        let mut cut = [0usize; 4];
        count_patterns(&data, 20, &mut cut);
        assert_eq!(cut, [1, 0, 0, 0], "only what starts before the limit");
    }

    #[test]
    fn this_program_names_its_own_modules_and_its_own_file_range() {
        // the test program is a PE file under Wine: the checks that read real memory run without faulting
        let base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
        assert_ne!(base, 0);
        let size = memscan::read(base, 0x400).and_then(|h| size_of_image(&h));
        assert!(size.is_some_and(|s| s > 0x1000), "{size:?}");
        let name = module_of(base + 0x1000).expect("the module that holds the program's code");
        assert!(name.to_lowercase().ends_with(".exe"), "{name}");
        // a whole look runs to the end and writes its lines
        let dir = std::env::temp_dir().join(format!("ashen-me2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = Logger::open(&dir.join("me2-hook.txt"), "t");
        check(&log, "a test");
        let text = std::fs::read_to_string(dir.join("me2-hook.txt")).unwrap();
        assert!(text.contains("--- a test ---") && text.contains("modengine2.dll is not loaded") && text.contains("the program file is at 0x") && text.contains("as ModEngine2 patches it: found "), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
