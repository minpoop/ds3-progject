//! Small helpers shared by the parsers: bounds-checked reads (every read of untrusted bytes goes through these, so a
//! damaged file gives `None`/an error and never a panic) and a few formatting helpers.

/// `len` bytes at `at`, or `None` if they are not all inside `b` (also when `at + len` would overflow).
pub(crate) fn get(b: &[u8], at: usize, len: usize) -> Option<&[u8]> {
    let end = at.checked_add(len)?;
    b.get(at..end)
}

pub(crate) fn u16_le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(get(b, at, 2)?.try_into().ok()?))
}

pub(crate) fn u32_le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(get(b, at, 4)?.try_into().ok()?))
}

pub(crate) fn i32_le(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(get(b, at, 4)?.try_into().ok()?))
}

pub(crate) fn i64_le(b: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_le_bytes(get(b, at, 8)?.try_into().ok()?))
}

pub(crate) fn u32_be(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(get(b, at, 4)?.try_into().ok()?))
}

/// Overwrite `bytes.len()` bytes at `at`; `None` if that does not fit.
pub(crate) fn put(out: &mut [u8], at: usize, bytes: &[u8]) -> Option<()> {
    let end = at.checked_add(bytes.len())?;
    out.get_mut(at..end)?.copy_from_slice(bytes);
    Some(())
}

/// The smallest multiple of `align` (a power of two, at least 1) that is `>= v`.
pub(crate) fn align_up(v: u64, align: u64) -> Option<u64> {
    debug_assert!(align.is_power_of_two());
    v.checked_add(align - 1).map(|x| x & !(align - 1))
}

/// The SHA-256 of `data` as lower-case hexadecimal.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(data))
}

/// Lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

/// The first `max` characters of `s` (with `...` after them if it was cut), for report lines. Control characters are shown
/// as `\n`, `\r`, `\t` or `?` so a snippet always stays on one line.
pub fn snippet(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i == max {
            out.push_str("...");
            break;
        }
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_bounds_checked() {
        let b = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
        assert_eq!(u32_le(&b, 0), Some(0x04030201));
        assert_eq!(u32_le(&b, 5), Some(0x09080706));
        assert_eq!(u32_le(&b, 6), None);
        assert_eq!(i64_le(&b, 1), Some(0x0908070605040302));
        assert_eq!(i64_le(&b, 2), None);
        assert_eq!(u32_le(&b, usize::MAX), None, "no overflow panic");
        assert_eq!(get(&b, usize::MAX - 1, 4), None);
        assert_eq!(u32_be(&b, 0), Some(0x01020304));
        assert_eq!(i32_le(&[0xFF, 0xFF, 0xFF, 0xFF], 0), Some(-1));
        assert_eq!(i64_le(&[0xFF; 8], 0), Some(-1));
        assert_eq!(u16_le(&b, 7), Some(0x0908));
        assert_eq!(u16_le(&b, 8), None);
    }

    #[test]
    fn put_and_align() {
        let mut b = [0u8; 6];
        assert_eq!(put(&mut b, 2, &[9, 9]), Some(()));
        assert_eq!(b, [0, 0, 9, 9, 0, 0]);
        assert_eq!(put(&mut b, 5, &[1, 2]), None);
        assert_eq!(put(&mut b, usize::MAX, &[1]), None);
        assert_eq!(align_up(0, 16), Some(0));
        assert_eq!(align_up(1, 16), Some(16));
        assert_eq!(align_up(16, 16), Some(16));
        assert_eq!(align_up(17, 16), Some(32));
        assert_eq!(align_up(u64::MAX, 16), None);
        assert_eq!(align_up(5, 1), Some(5));
    }

    #[test]
    fn hex_and_snippets() {
        assert_eq!(hex(&[0, 1, 0xab, 0xff]), "0001abff");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(snippet("short", 10), "short");
        assert_eq!(snippet("exactly ten", 11), "exactly ten");
        assert_eq!(snippet("a longer text than allowed", 8), "a longer...");
        assert_eq!(snippet("two\nlines\r\there", 40), "two\\nlines\\r\\there");
        assert_eq!(snippet("bell\u{7}", 10), "bell?");
    }
}
