//! Where does Dark Souls III keep the text of its item names? The first real runs found each name exactly once in
//! memory but not inside any text table in the shape of an `.fmg` file. This looks closer, read-only: for a few item
//! names it shows the memory around the string (as text and as bytes) and finds every place in memory that holds a
//! pointer to the string, with the words around that pointer, so the structure that holds the names (an array of
//! pointers? a map?) can be worked out offline from the log.
use super::memscan;
use ashen_common::logging::Logger;
use std::time::Duration;

/// Whole item names (English game) that occur once or a few times in memory.
pub const ANCHORS: [&str; 6] = ["Longsword", "Shortsword", "Knight Shield", "Light Crossbow", "Standard Bolt", "Estus Flask"];
/// The first this many anchors also get the (slower) pointer scan.
const POINTER_SCAN_ANCHORS: usize = 5;

/// Bytes as UTF-16 text, anything unprintable shown as a dot.
pub fn utf16_view(bytes: &[u8]) -> String {
    bytes
        .chunks_exact(2)
        .map(|c| {
            let u = u16::from_le_bytes([c[0], c[1]]) as u32;
            match char::from_u32(u) {
                Some(ch) if (' '..='~').contains(&ch) => ch,
                Some(ch) if u > 0xFF && ch.is_alphabetic() => ch,
                _ => '\u{b7}',
            }
        })
        .collect()
}

/// A plausible UTF-16 string at `addr` (at least 3 printable ASCII characters, ended by NUL or cut at 40), if any.
pub fn string_at(addr: usize) -> Option<String> {
    if addr < 0x10000 {
        return None;
    }
    let mut b = [0u8; 80];
    let n = memscan::read_into(addr, &mut b);
    let mut s = String::new();
    for c in b[..n & !1].chunks_exact(2) {
        let u = u16::from_le_bytes([c[0], c[1]]);
        if u == 0 {
            break;
        }
        match char::from_u32(u as u32) {
            Some(ch) if (' '..='~').contains(&ch) => s.push(ch),
            _ => return None,
        }
    }
    (s.chars().count() >= 3).then_some(s)
}

fn show_neighbourhood(log: &Logger, name: &str, addr: usize) {
    let before = 192.min(addr.saturating_sub(0x10000));
    let Some(bytes) = memscan::read(addr - before, before + 192) else { return };
    log.log(&format!("    {name:?} at 0x{addr:X}: {} bytes before: {:02x?}", before.min(24), &bytes[before - before.min(24)..before]));
    log.log(&format!("      text around it: {}", utf16_view(&bytes[..bytes.len() & !1])));
}

fn show_words(log: &Logger, found_at: usize, target: usize, name: &str, delta: isize) {
    let base = found_at.saturating_sub(96);
    let Some(bytes) = memscan::read(base, 192) else { return };
    log.log(&format!("    pointer to {name:?}{} found at 0x{found_at:X}; the words around it:", if delta == 0 { "" } else { " (to the 8 bytes before the text)" }));
    for i in 0..24 {
        let v = u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        let a = base + i * 8;
        let mut line = format!("      {:+4}  0x{v:016X}", a as isize - found_at as isize);
        if v < 0x1_0000_0000 {
            line.push_str(&format!("  ({} / {} as 32-bit halves: {}, {})", v, v as u32, v as u32, (v >> 32) as u32));
        } else if let Some(s) = string_at(v as usize) {
            line.push_str(&format!("  -> {s:?}"));
        } else if let Some(s) = string_at(v as usize + 8) {
            line.push_str(&format!("  -> (+8) {s:?}"));
        }
        if a == found_at {
            line.push_str("   <== here");
        }
        log.log(&line);
    }
    let _ = target;
}

/// The whole diagnosis. Takes a while (a few full scans of the address space), so it runs on the probe thread.
pub fn run(log: &Logger, round: usize) {
    log.log(&format!("--- text diagnosis {round} ---"));
    let mut firsts: Vec<(&str, usize)> = Vec::new();
    for (k, name) in ANCHORS.iter().enumerate() {
        let hits = memscan::find_exact_utf16(name, Duration::from_secs(25));
        let shown: Vec<String> = hits.iter().take(6).map(|h| format!("0x{h:X}")).collect();
        log.log(&format!("  {name:?}: {} whole-string occurrence(s) {}", hits.len(), shown.join(" ")));
        for h in hits.iter().take(2) {
            show_neighbourhood(log, name, *h);
        }
        if k < POINTER_SCAN_ANCHORS {
            if let Some(h) = hits.first() {
                firsts.push((name, *h));
            }
        }
    }
    // who points at them? the string itself, or the 8 bytes in front of it (where the game keeps a small header)
    let mut targets: Vec<usize> = Vec::new();
    for (_, h) in &firsts {
        targets.push(*h);
        targets.push(h - 8);
    }
    if targets.is_empty() {
        log.log("  no item name was found, so there is nothing to look for pointers to");
        return;
    }
    let refs = memscan::find_pointers(&targets, Duration::from_secs(40));
    log.log(&format!("  pointer scan: {} place(s) hold a pointer to one of the {} strings", refs.len(), firsts.len()));
    for (name, h) in &firsts {
        for (delta, t) in [(0isize, *h), (-8, h - 8)] {
            let mine: Vec<&(usize, usize)> = refs.iter().filter(|(_, target)| *target == t).collect();
            log.log(&format!("  {name:?}: {} pointer(s) to {}", mine.len(), if delta == 0 { "the text" } else { "the header in front of the text" }));
            for (at, target) in mine.into_iter().take(3) {
                show_words(log, *at, *target, name, delta);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_text_neighbourhoods_and_pointers_to_a_string_in_this_process() {
        // a small "array of pointers to strings", like a map of item names might be
        let names = ["Zzqxy Alpha", "Zzqxy Beta", "Zzqxy Gamma"];
        let strings: Vec<Vec<u16>> = names.iter().map(|n| n.encode_utf16().chain([0, 0, 0, 0]).collect()).collect();
        let table: Vec<u64> = strings.iter().map(|s| s.as_ptr() as u64).collect();
        let dir = std::env::temp_dir().join(format!("ashen-textdiag-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = Logger::open(&dir.join("log.txt"), "t");
        let hit = memscan::find_exact_utf16("Zzqxy Beta", Duration::from_secs(60));
        assert!(hit.contains(&(strings[1].as_ptr() as usize)), "the string is found by its exact text");
        let refs = memscan::find_pointers(&[strings[1].as_ptr() as usize], Duration::from_secs(60));
        assert!(refs.iter().any(|(at, _)| *at == table.as_ptr() as usize + 8), "the pointer table entry is found: {refs:x?}");
        show_words(&log, table.as_ptr() as usize + 8, strings[1].as_ptr() as usize, "Zzqxy Beta", 0);
        show_neighbourhood(&log, "Zzqxy Beta", strings[1].as_ptr() as usize);
        let text = std::fs::read_to_string(dir.join("log.txt")).unwrap();
        assert!(text.contains("-> \"Zzqxy Alpha\"") && text.contains("-> \"Zzqxy Gamma\"") && text.contains("<== here"), "{text}");
        assert!(text.contains("Zzqxy Beta"), "{text}");
        drop((strings, table));
    }

    #[test]
    fn views_and_strings_are_safe_on_garbage() {
        assert_eq!(utf16_view(&[0x41, 0, 0x00, 0x00, 0xFF, 0xFF]), "A\u{b7}\u{b7}");
        assert!(string_at(0).is_none());
        assert!(string_at(0x20000000000000).is_none());
        let s: Vec<u16> = "Hello".encode_utf16().chain([0]).collect();
        assert_eq!(string_at(s.as_ptr() as usize).as_deref(), Some("Hello"));
        let short: Vec<u16> = "Hi".encode_utf16().chain([0]).collect();
        assert!(string_at(short.as_ptr() as usize).is_none(), "two characters are not enough to call it text");
    }
}
