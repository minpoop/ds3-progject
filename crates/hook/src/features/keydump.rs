//! Public RSA keys, as plain PEM text, from this process's memory. Dark Souls III's archives (`Data0.bhd` ...) have their
//! tables of contents encrypted with RSA keys that the game carries as text; `ashenmarine-setup ds3-prepare` needs them to
//! read the item names out of the player's own install. Normally it finds them in the game's program file; if that file
//! does not hold them as plain text (the running game does, or it could not read the archives), this writes them to
//! `cache/ds3-keys.pem` so the setup tool can use them. Public keys only: nothing here looks at anything private.
//! Read-only on the game; the only thing written is that one small file.
use super::memscan;
use ashen_common::logging::Logger;
use std::path::Path;
use std::time::{Duration, Instant};

const BEGINS: [&[u8]; 2] = [b"-----BEGIN RSA PUBLIC KEY-----", b"-----BEGIN PUBLIC KEY-----"];
/// A 2048-bit key is about 450 characters; anything longer than this is not one.
const MAX_BLOCK: usize = 2048;

fn end_marker(begin: &[u8]) -> Vec<u8> {
    let mut e = b"-----END".to_vec();
    e.extend_from_slice(&begin[b"-----BEGIN".len()..]);
    e
}

/// Every complete PEM public-key block in `buf` (as text, one per element), in the order found.
pub fn extract_pem_blocks(buf: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for begin in BEGINS {
        let end = end_marker(begin);
        let finder = memchr::memmem::Finder::new(begin);
        for at in finder.find_iter(buf) {
            let body_start = at + begin.len();
            let window = &buf[body_start..buf.len().min(body_start + MAX_BLOCK)];
            let Some(rel) = memchr::memmem::find(window, &end) else { continue };
            let body = &window[..rel];
            // only base64 and line breaks may sit between the markers
            if body.is_empty() || !body.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'=' | b'\r' | b'\n')) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(&buf[at..body_start + rel + end.len()]) else { continue };
            out.push(text.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }
    out
}

/// Scan the whole address space (within `budget`) and write what was found to `out` (merged with what is there).
/// Returns how many distinct keys the file holds afterwards.
pub fn run(log: &Logger, out: &Path, budget: Duration) -> usize {
    const CHUNK: usize = 8 << 20;
    let started = Instant::now();
    let mut blocks: Vec<String> = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    let overlap = MAX_BLOCK + 64;
    'regions: for r in memscan::regions(1 << 30) {
        let mut off = 0usize;
        while off < r.size {
            if started.elapsed() > budget {
                log.log("key scan: out of time; what was found so far is kept");
                break 'regions;
            }
            let want = CHUNK.min(r.size - off);
            let got = memscan::read_into(r.base + off, &mut buf[..want]);
            for b in extract_pem_blocks(&buf[..got]) {
                if !blocks.contains(&b) {
                    blocks.push(b);
                }
            }
            if off + want >= r.size {
                break;
            }
            off += (want - overlap).max(1);
        }
    }
    log.log(&format!("key scan: {} public key block(s) in memory ({:.1} s)", blocks.len(), started.elapsed().as_secs_f32()));
    if blocks.is_empty() {
        return 0;
    }
    // keep what an earlier run saved
    if let Ok(old) = std::fs::read_to_string(out) {
        for b in extract_pem_blocks(old.as_bytes()) {
            if !blocks.contains(&b) {
                blocks.push(b);
            }
        }
    }
    let mut text = String::new();
    for b in &blocks {
        text.push_str(b);
        text.push('\n');
    }
    if let Some(dir) = out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(out, text) {
        Ok(()) => log.log(&format!("key scan: {} key(s) written to {}", blocks.len(), out.display())),
        Err(e) => log.log(&format!("key scan: could not write {}: {e}", out.display())),
    }
    blocks.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEM: &str = "-----BEGIN RSA PUBLIC KEY-----\nMIIBCgKCAQEAtestonlytestonlytestonly0123456789+/abcdefgh\nijklmnopqrstuvwxyzABCDEFGH==\n-----END RSA PUBLIC KEY-----";

    #[test]
    fn finds_a_block_between_junk_and_keeps_its_text() {
        let mut buf = b"\x00\x01junk".to_vec();
        buf.extend_from_slice(PEM.as_bytes());
        buf.extend_from_slice(b"\x00\x00tail");
        assert_eq!(extract_pem_blocks(&buf), vec![PEM.to_string()]);
    }

    #[test]
    fn crlf_line_ends_become_plain_ones_and_both_marker_kinds_are_found() {
        let crlf = PEM.replace('\n', "\r\n");
        assert_eq!(extract_pem_blocks(crlf.as_bytes()), vec![PEM.to_string()]);
        let spki = "-----BEGIN PUBLIC KEY-----\nQUJDREVGR0hJSktMTU5PUA==\n-----END PUBLIC KEY-----";
        let both = format!("{PEM}\0\0{spki}");
        assert_eq!(extract_pem_blocks(both.as_bytes()), vec![PEM.to_string(), spki.to_string()]);
    }

    #[test]
    fn text_that_only_mentions_the_marker_is_not_a_key() {
        assert!(extract_pem_blocks(b"-----BEGIN RSA PUBLIC KEY----- is how a key starts").is_empty(), "no end marker");
        let junk = "-----BEGIN RSA PUBLIC KEY-----\n\u{1}\u{2}binary junk here\n-----END RSA PUBLIC KEY-----";
        assert!(extract_pem_blocks(junk.as_bytes()).is_empty(), "not base64 between the markers");
        assert!(extract_pem_blocks(b"-----BEGIN RSA PUBLIC KEY----------END RSA PUBLIC KEY-----").is_empty(), "empty body");
        let mut long = b"-----BEGIN RSA PUBLIC KEY-----".to_vec();
        long.extend(std::iter::repeat_n(b'A', 5000));
        long.extend_from_slice(b"-----END RSA PUBLIC KEY-----");
        assert!(extract_pem_blocks(&long).is_empty(), "far too long to be a key");
    }

    #[test]
    fn a_scan_of_this_process_finds_a_key_held_in_memory_and_merges_with_the_old_file() {
        // the text sits in this test's own heap, so the whole-process scan must find it
        let held: Vec<u8> = PEM.as_bytes().to_vec();
        let dir = std::env::temp_dir().join(format!("ashen-keydump-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = Logger::open(&dir.join("log.txt"), "t");
        let out = dir.join("cache").join("ds3-keys.pem");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let older = "-----BEGIN RSA PUBLIC KEY-----\nQUJDREVGRw==\n-----END RSA PUBLIC KEY-----\n";
        std::fs::write(&out, older).unwrap();
        let n = run(&log, &out, Duration::from_secs(60));
        assert!(n >= 2, "{n}");
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("testonlytestonly") && text.contains("QUJDREVGRw=="), "{text}");
        drop(held);
    }
}
