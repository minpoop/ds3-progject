//! The 32-bit path hash Dark Souls III's archive headers use instead of file names.
//!
//! The archives (`Data0.bhd` ...) do not store names. A file is found by the hash of its path: the path is trimmed,
//! written with `/` instead of `\`, lower-cased and given a leading `/`, and the hash is `h = h * 37 + c` over its
//! UTF-16 code units, wrapping at 32 bits (as documented by the Souls modding community).

/// The path the way it is hashed: trimmed, `/` separators, lower case, leading `/`.
pub fn normalize_path(path: &str) -> String {
    let t = path.trim().replace('\\', "/").to_lowercase();
    if t.starts_with('/') {
        t
    } else {
        format!("/{t}")
    }
}

/// The archive hash of a path (see the module docs). `"msg\\ENGLISH\\Item.msgbnd.dcx"` and `"/msg/english/item.msgbnd.dcx"`
/// give the same value.
pub fn path_hash(path: &str) -> u32 {
    normalize_path(path).encode_utf16().fold(0u32, |h, c| h.wrapping_mul(37).wrapping_add(u32::from(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule written out differently from `path_hash` (64-bit arithmetic, per character), as a cross-check.
    fn slow_hash(path: &str) -> u32 {
        let mut h: u64 = 0;
        for c in normalize_path(path).chars() {
            let mut units = [0u16; 2];
            for u in c.encode_utf16(&mut units) {
                h = (h * 37 + u64::from(*u)) & 0xFFFF_FFFF;
            }
        }
        h as u32
    }

    #[test]
    fn known_values() {
        // worked out independently with python: h = (h * 37 + ord(c)) & 0xffffffff over "/" + path
        assert_eq!(path_hash(""), 47);
        assert_eq!(path_hash("/"), 47);
        assert_eq!(path_hash("a"), 47 * 37 + 97);
        assert_eq!(path_hash("/msg/english/item.msgbnd.dcx"), 0x50b4_24bf);
        assert_eq!(path_hash("/msg/english/menu.msgbnd.dcx"), 0x76ea_6189);
        assert_eq!(path_hash("/regulation.bin"), 0xe900_efac);
        assert_eq!(path_hash("/parts/wp_a_0200.partsbnd.dcx"), 0x7771_d5d1);
        assert_eq!(path_hash("/parts/wp_a_0200_l.partsbnd.dcx"), 0x8c75_4524);
    }

    #[test]
    fn spelling_does_not_matter() {
        let a = path_hash("/msg/ENGLISH/item.msgbnd.dcx");
        assert_eq!(a, 0x50b4_24bf);
        assert_eq!(a, path_hash("msg/english/item.msgbnd.dcx"));
        assert_eq!(a, path_hash("msg\\ENGLISH\\ITEM.msgbnd.dcx"));
        assert_eq!(a, path_hash("  \\msg\\english\\item.msgbnd.dcx \n"));
        assert_ne!(a, path_hash("/msg/english/item.msgbnd"));
        assert_eq!(normalize_path("Parts\\WP_A_0200.partsbnd.dcx"), "/parts/wp_a_0200.partsbnd.dcx");
    }

    #[test]
    fn matches_the_slow_version_and_wraps() {
        for p in [
            "/a",
            "/parts/wp_a_0200.partsbnd.dcx",
            "/regulation.bin",
            "/map/m30_00_00_00/m30_00_00_00.mapbnd.dcx",
            "/this/is/a/very/long/path/that/certainly/overflows/thirty/two/bits/when/multiplied/by/37/over/and/over.bin",
            "/\u{e9}\u{3042}\u{1f600}.bin",
        ] {
            assert_eq!(path_hash(p), slow_hash(p), "{p}");
        }
        // 6 characters are already more than 2^32 / 37: a long path must wrap rather than panic in debug builds
        assert_eq!(path_hash("/zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"), slow_hash("/zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
    }
}
