//! Offline guard rules: which network destinations the game process may touch.
//!
//! Only loopback is allowed (127.0.0.0/8, ::1, and IPv4-mapped loopback). Host names are allowed only when
//! they are empty, `localhost`, or a literal IP address (a literal needs no lookup; the later connect or
//! send is judged by `sockaddr_denied`).

pub const AF_INET: u16 = 2;
pub const AF_INET6: u16 = 23;

pub fn ipv4_is_loopback(b: [u8; 4]) -> bool {
    b[0] == 127
}

pub fn ipv6_is_loopback(b: [u8; 16]) -> bool {
    let mut loopback = [0u8; 16];
    loopback[15] = 1;
    if b == loopback {
        return true;
    }
    // ::ffff:127.x.x.x
    b[..10] == [0; 10] && b[10] == 0xff && b[11] == 0xff && b[12] == 127
}

/// Judge a `sockaddr` given as bytes. Returns true when the destination must be refused.
/// Non-IP families (unspecified, unix) are allowed: they cannot leave the machine.
pub fn sockaddr_denied(bytes: &[u8]) -> bool {
    if bytes.len() < 2 {
        return false;
    }
    let family = u16::from_le_bytes([bytes[0], bytes[1]]);
    match family {
        AF_INET => {
            if bytes.len() < 8 {
                return true; // malformed IPv4 address: refuse
            }
            !ipv4_is_loopback([bytes[4], bytes[5], bytes[6], bytes[7]])
        }
        AF_INET6 => {
            if bytes.len() < 24 {
                return true;
            }
            let mut a = [0u8; 16];
            a.copy_from_slice(&bytes[8..24]);
            !ipv6_is_loopback(a)
        }
        _ => false,
    }
}

/// Same as [`sockaddr_denied`] for a raw pointer. Reads only as many bytes as the family needs.
///
/// # Safety
/// `p` must be null or point to a valid `sockaddr` of the family it declares.
pub unsafe fn sockaddr_denied_ptr(p: *const u8) -> bool {
    if p.is_null() {
        return false;
    }
    let family = core::ptr::read_unaligned(p as *const u16);
    let need = match family {
        AF_INET => 8,
        AF_INET6 => 24,
        _ => return false,
    };
    let mut buf = [0u8; 24];
    core::ptr::copy_nonoverlapping(p, buf.as_mut_ptr(), need);
    sockaddr_denied(&buf[..need])
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut n = 0;
    for part in s.split('.') {
        if n == 4 || part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        out[n] = part.parse::<u16>().ok().filter(|v| *v <= 255)? as u8;
        n += 1;
    }
    if n == 4 {
        Some(out)
    } else {
        None
    }
}

fn is_ip_literal(s: &str) -> bool {
    let t = s.trim_start_matches('[').trim_end_matches(']');
    parse_ipv4(t).is_some() || (t.contains(':') && t.bytes().all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.' || b == b'%'))
}

/// True when resolving/connecting to this host name may proceed.
pub fn name_is_allowed(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return true;
    }
    let l = n.to_ascii_lowercase();
    l == "localhost" || l == "localhost." || is_ip_literal(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(a: [u8; 4], port: u16) -> Vec<u8> {
        let mut b = vec![0u8; 16];
        b[..2].copy_from_slice(&AF_INET.to_le_bytes());
        b[2..4].copy_from_slice(&port.to_be_bytes());
        b[4..8].copy_from_slice(&a);
        b
    }

    fn v6(a: [u8; 16]) -> Vec<u8> {
        let mut b = vec![0u8; 28];
        b[..2].copy_from_slice(&AF_INET6.to_le_bytes());
        b[8..24].copy_from_slice(&a);
        b
    }

    #[test]
    fn ipv4_loopback_allowed_everything_else_denied() {
        assert!(!sockaddr_denied(&v4([127, 0, 0, 1], 80)));
        assert!(!sockaddr_denied(&v4([127, 1, 2, 3], 80)));
        assert!(sockaddr_denied(&v4([8, 8, 8, 8], 53)));
        assert!(sockaddr_denied(&v4([192, 168, 1, 10], 80)), "LAN is not loopback");
        assert!(sockaddr_denied(&v4([0, 0, 0, 0], 80)));
        assert!(sockaddr_denied(&v4([255, 255, 255, 255], 80)), "broadcast");
    }

    #[test]
    fn ipv6_rules() {
        let mut lo = [0u8; 16];
        lo[15] = 1;
        assert!(!sockaddr_denied(&v6(lo)));
        let mut mapped = [0u8; 16];
        mapped[10] = 0xff;
        mapped[11] = 0xff;
        mapped[12..].copy_from_slice(&[127, 0, 0, 1]);
        assert!(!sockaddr_denied(&v6(mapped)));
        mapped[12..].copy_from_slice(&[1, 2, 3, 4]);
        assert!(sockaddr_denied(&v6(mapped)));
        assert!(sockaddr_denied(&v6([0x20, 0x01, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])));
    }

    #[test]
    fn other_families_and_short_buffers() {
        assert!(!sockaddr_denied(&[]));
        assert!(!sockaddr_denied(&[0, 0]), "AF_UNSPEC (used to disconnect a UDP socket)");
        assert!(!sockaddr_denied(&[1, 0, b'x', 0]), "AF_UNIX");
        assert!(sockaddr_denied(&[2, 0, 0, 80]), "truncated IPv4 is refused");
    }

    #[test]
    fn pointer_variant_matches() {
        let a = v4([8, 8, 4, 4], 443);
        assert!(unsafe { sockaddr_denied_ptr(a.as_ptr()) });
        let b = v4([127, 0, 0, 1], 443);
        assert!(!unsafe { sockaddr_denied_ptr(b.as_ptr()) });
        assert!(!unsafe { sockaddr_denied_ptr(core::ptr::null()) });
    }

    #[test]
    fn names() {
        for ok in ["", "  ", "localhost", "LOCALHOST", "localhost.", "127.0.0.1", "8.8.8.8", "::1", "[::1]", "2001:db8::1"] {
            assert!(name_is_allowed(ok), "{ok:?} should be allowed");
        }
        for bad in ["example.com", "ds3.fromsoftware-game.net", "1.2.3", "999.1.1.1.1", "my-pc", "local.host"] {
            assert!(!name_is_allowed(bad), "{bad:?} should be refused");
        }
    }
}
