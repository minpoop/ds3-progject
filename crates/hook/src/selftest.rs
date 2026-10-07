//! In-process proof that the sandbox works, run right after the hooks are enabled and before the game can
//! reach its save. If either test fails the game is closed (see `init::fatal`).
//!
//! Neither test ever writes into the player's real save folder.

use ashen_common::config::HookConfig;
use core::mem::{size_of, zeroed};
use std::fs;
use std::path::Path;
use windows_sys::Win32::Networking::WinSock::{closesocket, connect, socket, WSACleanup, WSAGetLastError, WSAStartup, AF_INET, INVALID_SOCKET, IPPROTO_UDP, SOCKADDR, SOCK_DGRAM, WSADATA};

/// A file that exists only in the private copy must be visible through the real folder's path.
pub fn redirect(cfg: &HookConfig) -> Result<(), String> {
    let sandbox = Path::new(&cfg.sandbox_save_dir);
    let probe_in_sandbox = sandbox.join(".ashen-selftest");
    fs::write(&probe_in_sandbox, b"ashen").map_err(|e| format!("cannot write the self-test file in the private save folder: {e}"))?;
    // Same name, addressed through the REAL folder. Only a working redirect can find it.
    let through_real = Path::new(&cfg.real_save_dir).join(".ashen-selftest");
    let seen = fs::metadata(&through_real).is_ok();
    let _ = fs::remove_file(&probe_in_sandbox);
    if seen {
        Ok(())
    } else {
        Err("the save-folder redirect is not working (self-test file not found through the real path)".into())
    }
}

/// A UDP `connect` to a documentation-range address (192.0.2.1, RFC 5737) must be refused by the guard
/// with WSAECONNREFUSED (10061). Without the guard it succeeds or fails with a different error.
pub fn network() -> Result<String, String> {
    unsafe {
        let mut data: WSADATA = zeroed();
        if WSAStartup(0x0202, &mut data) != 0 {
            return Ok("network self-test skipped: WSAStartup failed (no usable network stack)".into());
        }
        let s = socket(AF_INET as i32, SOCK_DGRAM, IPPROTO_UDP);
        if s == INVALID_SOCKET {
            WSACleanup();
            return Ok("network self-test skipped: cannot create a socket (no usable network stack)".into());
        }
        let mut addr = [0u8; 16];
        addr[..2].copy_from_slice(&(AF_INET as u16).to_le_bytes());
        addr[2..4].copy_from_slice(&9u16.to_be_bytes());
        addr[4..8].copy_from_slice(&[192, 0, 2, 1]);
        let rc = connect(s, addr.as_ptr() as *const SOCKADDR, size_of::<[u8; 16]>() as i32);
        let err = WSAGetLastError();
        closesocket(s);
        WSACleanup();
        if rc != 0 && err == 10061 {
            Ok("network self-test passed: non-loopback connect refused (WSAECONNREFUSED)".into())
        } else {
            Err(format!("the offline guard is not working (connect to 192.0.2.1 returned {rc}, error {err}; expected refusal 10061)"))
        }
    }
}
