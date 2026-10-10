//! Test harness (Windows target; run under Wine). One binary, three personalities chosen by file name / argument:
//!
//!   ashen-harness.exe probe ...        load the hook DLL and try to break out of the sandbox
//!   modengine2_launcher.exe ...        stand-in for ModEngine2's launcher: starts the "game", then exits
//!   DarkSoulsIII.exe                   stand-in for the game: loads the hook DLL like ModEngine2 would, "plays", exits
//!
//! Output is `PASS name` / `FAIL name: detail` lines and a `SUMMARY` line.
#![allow(clippy::missing_safety_doc)]

use ashen_common::config::HookConfig;
use std::ffi::c_void;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

#[link(name = "ws2_32")]
extern "system" {
    fn WSAStartup(ver: u16, data: *mut [u8; 512]) -> i32;
    fn WSACleanup() -> i32;
    fn WSAGetLastError() -> i32;
    fn socket(af: i32, ty: i32, proto: i32) -> usize;
    fn closesocket(s: usize) -> i32;
    fn connect(s: usize, name: *const u8, len: i32) -> i32;
    fn sendto(s: usize, buf: *const u8, len: i32, flags: i32, to: *const u8, tolen: i32) -> i32;
    fn getaddrinfo(node: *const u8, service: *const u8, hints: *const c_void, res: *mut *mut c_void) -> i32;
    fn GetAddrInfoW(node: *const u16, service: *const u16, hints: *const c_void, res: *mut *mut c_void) -> i32;
    fn gethostbyname(name: *const u8) -> *mut c_void;
    fn WSAIoctl(s: usize, code: u32, inb: *mut c_void, cbin: u32, outb: *mut c_void, cbout: u32, ret: *mut u32, ov: *mut c_void, cr: *mut c_void) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
    fn GetLastError() -> u32;
    fn SetLastError(code: u32);
    fn Sleep(ms: u32);
    fn GetFileAttributesExW(name: *const u16, level: u32, info: *mut [u8; 36]) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: *mut c_void, menu: *mut c_void, inst: *mut c_void, param: *mut c_void) -> *mut c_void;
    fn PeekMessageW(msg: *mut [u8; 64], hwnd: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
    fn DispatchMessageW(msg: *const [u8; 64]) -> isize;
    fn GetWindowTextW(hwnd: *mut c_void, buf: *mut u16, max: i32) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn sockaddr_in(ip: [u8; 4], port: u16) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[..2].copy_from_slice(&2u16.to_le_bytes());
    a[2..4].copy_from_slice(&port.to_be_bytes());
    a[4..8].copy_from_slice(&ip);
    a
}

#[derive(Default)]
struct Report {
    pass: u32,
    fail: u32,
}

impl Report {
    fn check(&mut self, name: &str, ok: bool, detail: impl AsRef<str>) {
        if ok {
            self.pass += 1;
            println!("PASS {name}");
        } else {
            self.fail += 1;
            println!("FAIL {name}: {}", detail.as_ref());
        }
    }
    fn summary(&self) -> ExitCode {
        println!("SUMMARY pass={} fail={}", self.pass, self.fail);
        if self.fail == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        }
    }
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// Wait until the hook's log says READY. Returns false on timeout.
fn wait_ready(log: &Path, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if fs::read_to_string(log).map(|t| t.contains("READY:")).unwrap_or(false) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn load_dll(dll: &str) -> bool {
    unsafe { !LoadLibraryW(wide(dll).as_ptr()).is_null() }
}

// ============================================================================================ probe

fn probe(args: &[String]) -> ExitCode {
    let dll = arg_value(args, "--dll").expect("--dll");
    let block = arg_value(args, "--block").map(|v| v == "true").unwrap_or(true);
    let dll_path = PathBuf::from(&dll);
    let cfg = match HookConfig::load(&dll_path.parent().unwrap().join("config").join("ashenmarine.json")) {
        Ok(c) => c,
        Err(_) => {
            // Fail-closed scenario: the hook has no config, so it must close this process. Surviving is the failure.
            let mut r = Report::default();
            r.check("dll-loads", load_dll(&dll), "LoadLibraryW failed");
            std::thread::sleep(Duration::from_secs(8));
            r.check("process-was-closed-by-the-hook", false, "still alive 8 s after loading the hook with no config");
            return r.summary();
        }
    };
    let mut r = Report::default();

    r.check("dll-loads", load_dll(&dll), "LoadLibraryW failed");
    r.check("hook-ready", wait_ready(Path::new(&cfg.log_file), 20), "no READY line in the hook log within 20 s");

    // ---------------------------------------------------------------- files: everything under the REAL path must land in the private copy
    let real = PathBuf::from(&cfg.real_save_dir);
    let dir = real.join("probe-id");
    r.check("fs-create-dir", fs::create_dir_all(&dir).is_ok(), "create_dir_all through the real path failed");
    r.check("fs-write", fs::write(dir.join("DS30000.sl2"), b"hello save").is_ok(), "write failed");
    r.check("fs-read-back", fs::read(dir.join("DS30000.sl2")).map(|b| b == b"hello save").unwrap_or(false), "read back differs");
    r.check("fs-metadata", fs::metadata(dir.join("DS30000.sl2")).map(|m| m.len() == 10).unwrap_or(false), "metadata/attributes wrong");
    r.check("fs-list-dir", fs::read_dir(&dir).map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.file_name() == "DS30000.sl2")).unwrap_or(false), "directory listing did not show the file");
    r.check("fs-copy", fs::copy(dir.join("DS30000.sl2"), dir.join("DS30000.sl2.bak")).is_ok(), "copy failed");
    r.check("fs-rename", fs::rename(dir.join("DS30000.sl2.bak"), dir.join("renamed.bak")).is_ok(), "rename failed");
    r.check("fs-rename-visible", dir.join("renamed.bak").exists(), "renamed file not visible");
    r.check("fs-delete", fs::remove_file(dir.join("renamed.bak")).is_ok() && !dir.join("renamed.bak").exists(), "delete failed");
    r.check("fs-slash-variant", fs::write(PathBuf::from(cfg.real_save_dir.replace('\\', "/")).join("probe-id/slash.txt"), b"x").is_ok(), "forward-slash path failed");
    // Files elsewhere are left alone.
    let other = PathBuf::from(arg_value(args, "--other").expect("--other"));
    r.check("fs-other-untouched", fs::write(&other, b"plain").is_ok() && fs::read(&other).map(|b| b == b"plain").unwrap_or(false), "write outside the save folder broke");

    // ---------------------------------------------------------------- network
    unsafe {
        let mut data = [0u8; 512];
        let started = WSAStartup(0x0202, &mut data) == 0;
        r.check("net-wsastartup", started, "WSAStartup failed");
        if started {
            // UDP "connect" never sends a packet, so it is safe and instant either way.
            let s = socket(2, 2, 17);
            let a = sockaddr_in([192, 0, 2, 1], 9);
            let rc = connect(s, a.as_ptr(), 16);
            let err = WSAGetLastError();
            if block {
                r.check("net-connect-denied", rc != 0 && err == 10061, format!("rc={rc} err={err}"));
            } else {
                r.check("net-control-connect-not-denied", !(rc != 0 && err == 10061), format!("rc={rc} err={err}: guard acted although it is switched off"));
            }
            let a2 = sockaddr_in([8, 8, 8, 8], 53);
            let n = sendto(s, b"x".as_ptr(), 1, 0, a2.as_ptr(), 16);
            let err2 = WSAGetLastError();
            if block {
                r.check("net-sendto-denied", n < 0 && err2 == 10051, format!("n={n} err={err2}"));
            }
            closesocket(s);

            if block {
                // TCP connect is refused immediately too.
                let t = socket(2, 1, 6);
                let rc = connect(t, sockaddr_in([93, 184, 216, 34], 80).as_ptr(), 16);
                let err = WSAGetLastError();
                r.check("net-tcp-connect-denied", rc != 0 && err == 10061, format!("rc={rc} err={err}"));
                closesocket(t);

                let mut res: *mut c_void = core::ptr::null_mut();
                let rc = getaddrinfo(b"example.com\0".as_ptr(), core::ptr::null(), core::ptr::null(), &mut res);
                r.check("net-getaddrinfo-denied", rc == 11001, format!("rc={rc}"));
                let mut resw: *mut c_void = core::ptr::null_mut();
                let rc = GetAddrInfoW(wide("example.com").as_ptr(), core::ptr::null(), core::ptr::null(), &mut resw);
                r.check("net-getaddrinfow-denied", rc == 11001, format!("rc={rc}"));
                r.check("net-gethostbyname-denied", gethostbyname(b"example.com\0".as_ptr()).is_null() && WSAGetLastError() == 11001, format!("err={}", WSAGetLastError()));
                let mut res2: *mut c_void = core::ptr::null_mut();
                let rc = getaddrinfo(b"localhost\0".as_ptr(), core::ptr::null(), core::ptr::null(), &mut res2);
                r.check("net-localhost-allowed", rc == 0, format!("rc={rc}"));

                // ConnectEx comes through WSAIoctl, not an export: the guard must wrap it.
                let guid: [u8; 16] = [0xb9, 0x07, 0xa2, 0x25, 0xf3, 0xdd, 0x60, 0x46, 0x8e, 0xe9, 0x76, 0xe5, 0x8c, 0x74, 0x06, 0x3e];
                let mut gb = guid;
                let mut fnptr: *mut c_void = core::ptr::null_mut();
                let mut got = 0u32;
                let ts = socket(2, 1, 6);
                let ok = WSAIoctl(ts, 0xC800_0006, gb.as_mut_ptr() as *mut c_void, 16, &mut fnptr as *mut _ as *mut c_void, 8, &mut got, core::ptr::null_mut(), core::ptr::null_mut());
                if ok == 0 && !fnptr.is_null() {
                    type ConnectEx = unsafe extern "system" fn(usize, *const u8, i32, *mut c_void, u32, *mut u32, *mut c_void) -> i32;
                    let f: ConnectEx = core::mem::transmute(fnptr);
                    let mut ov = [0u8; 32];
                    let a = sockaddr_in([93, 184, 216, 34], 80);
                    let rc = f(ts, a.as_ptr(), 16, core::ptr::null_mut(), 0, core::ptr::null_mut(), ov.as_mut_ptr() as *mut c_void);
                    let err = WSAGetLastError();
                    r.check("net-connectex-denied", rc == 0 && err == 10061, format!("rc={rc} err={err}"));
                } else {
                    r.check("net-connectex-denied", false, format!("could not obtain ConnectEx (rc={ok})"));
                }
                closesocket(ts);

                // WinHTTP / WinINet: with a null session the real function fails with ERROR_INVALID_HANDLE (6);
                // the guard answers 12029 for a non-local name first.
                for (module, func) in [("winhttp.dll", "WinHttpConnect\0"), ("wininet.dll", "InternetConnectW\0")] {
                    let m = LoadLibraryW(wide(module).as_ptr());
                    let p = if m.is_null() { core::ptr::null_mut() } else { GetProcAddress(m, func.as_ptr()) };
                    if p.is_null() {
                        println!("SKIP http-{func}: {module} not available here");
                        continue;
                    }
                    SetLastError(0);
                    let h = if module == "winhttp.dll" {
                        let f: unsafe extern "system" fn(isize, *const u16, u16, u32) -> isize = core::mem::transmute(p);
                        f(0, wide("example.com").as_ptr(), 80, 0)
                    } else {
                        let f: unsafe extern "system" fn(isize, *const u16, u16, *const u16, *const u16, u32, u32, usize) -> isize = core::mem::transmute(p);
                        f(0, wide("example.com").as_ptr(), 80, core::ptr::null(), core::ptr::null(), 3, 0, 0)
                    };
                    let err = GetLastError();
                    r.check(&format!("http-{}-denied", func.trim_end_matches('\0')), h == 0 && err == 12029, format!("h={h} err={err}"));
                }
            }
            WSACleanup();
        }
    }

    if block {
        // Loopback must keep working: a game may talk to itself.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut b = [0u8; 4];
                let _ = s.read_exact(&mut b);
                let _ = s.write_all(&b);
            }
        });
        let ok = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).and_then(|mut c| {
            c.write_all(b"ping")?;
            let mut b = [0u8; 4];
            c.read_exact(&mut b)?;
            Ok(b == *b"ping")
        });
        let detail = format!("{ok:?}");
        r.check("net-loopback-tcp-allowed", ok.unwrap_or(false), detail);
        let _ = t.join();
        let udp_ok = UdpSocket::bind("127.0.0.1:0").and_then(|a| {
            let b = UdpSocket::bind("127.0.0.1:0")?;
            b.send_to(b"u", a.local_addr()?)?;
            a.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut buf = [0u8; 1];
            a.recv_from(&mut buf).map(|(n, _)| n == 1)
        });
        let detail = format!("{udp_ok:?}");
        r.check("net-loopback-udp-allowed", udp_ok.unwrap_or(false), detail);
    }

    r.summary()
}

// ============================================================================================ stand-in for ModEngine2's launcher

fn fake_me2(args: &[String]) -> ExitCode {
    let exe = arg_value(args, "-p").expect("-p <game exe>");
    let cfg = arg_value(args, "-c").expect("-c <config>");
    let toml = fs::read_to_string(&cfg).unwrap_or_default();
    let dll = toml
        .lines()
        .find(|l| l.trim_start().starts_with("external_dlls"))
        .and_then(|l| l.split('"').nth(1))
        .unwrap_or("")
        .to_string();
    println!("fake modengine2: target={:?} game={exe} config={cfg} external_dll={dll}", arg_value(args, "-t"));
    if fs::metadata(&exe).is_err() {
        eprintln!("fake modengine2: game exe not found");
        return ExitCode::from(2);
    }
    if std::env::var("ASHEN_FAKE_SABOTAGE").as_deref() == Ok("delete-sandbox") {
        // Simulates the private save folder vanishing between the launcher and the hook.
        if let Ok(cfg) = HookConfig::load(&Path::new(&dll).parent().unwrap().join("config").join("ashenmarine.json")) {
            let _ = fs::remove_dir_all(&cfg.sandbox_save_dir);
            println!("fake modengine2: sabotage - removed {}", cfg.sandbox_save_dir);
        }
    }
    match Command::new(&exe).env("ASHEN_FAKE_DLL", &dll).spawn() {
        Ok(_) => ExitCode::SUCCESS, // like the real launcher: start the game, then go away
        Err(e) => {
            eprintln!("fake modengine2: cannot start the game: {e}");
            ExitCode::from(3)
        }
    }
}

// ============================================================================================ stand-in for the game

fn fake_ds3() -> ExitCode {
    let behavior = std::env::var("ASHEN_FAKE_BEHAVIOR").unwrap_or_default();
    if behavior == "sleep" {
        unsafe { Sleep(25_000) };
        return ExitCode::SUCCESS;
    }
    let dll = std::env::var("ASHEN_FAKE_DLL").unwrap_or_default();
    let result_path = PathBuf::from(std::env::var("ASHEN_FAKE_RESULT").unwrap_or_else(|_| "fake-ds3-result.txt".into()));
    let mut result = String::new();

    if behavior == "no-dll" {
        // Simulates the protection failing silently: the game never loads the hook DLL.
        result.push_str("dll=skipped\n");
    } else {
        if !load_dll(&dll) {
            let _ = fs::write(&result_path, "dll=failed-to-load\n");
            return ExitCode::from(11);
        }
        result.push_str("dll=loaded\n");
    }
    // The DLL decides whether this process survives. A game that is still running here is protected.
    let real = std::env::var("ASHEN_FAKE_REAL").expect("ASHEN_FAKE_REAL");
    let cfg_log = std::env::var("ASHEN_FAKE_LOG").unwrap_or_default();
    if behavior != "no-dll" && !cfg_log.is_empty() && !wait_ready(Path::new(&cfg_log), 20) {
        let _ = fs::write(&result_path, "ready=timeout\n");
        return ExitCode::from(12);
    }
    let id_dir = PathBuf::from(&real).join("76561198000000000");
    let _ = fs::create_dir_all(&id_dir);
    let wrote = fs::write(id_dir.join("DS30000.sl2"), format!("PLAYED {:?}", std::time::SystemTime::now())).is_ok();
    result.push_str(&format!("save_written={wrote}\n"));

    // A window, so the title marker can be checked.
    unsafe {
        let title = wide("DARK SOULS III");
        let hwnd = CreateWindowExW(0, wide("STATIC").as_ptr(), title.as_ptr(), 0x1000_0000 /* WS_VISIBLE */ | 0x00C0_0000 /* WS_CAPTION */, 10, 10, 300, 100, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
        let mut msg = [0u8; 64];
        let end = Instant::now() + Duration::from_secs(5);
        let mut text = String::new();
        while Instant::now() < end {
            while PeekMessageW(&mut msg, core::ptr::null_mut(), 0, 0, 1) != 0 {
                DispatchMessageW(&msg);
            }
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), 256);
            text = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if text.contains("Ashen Marine") {
                break;
            }
            Sleep(50);
        }
        result.push_str(&format!("title={text}\n"));
    }

    // Try to reach the outside world.
    unsafe {
        let mut data = [0u8; 512];
        WSAStartup(0x0202, &mut data);
        let s = socket(2, 2, 17);
        let rc = connect(s, sockaddr_in([8, 8, 8, 8], 53).as_ptr(), 16);
        result.push_str(&format!("net_connect_rc={rc} err={}\n", WSAGetLastError()));
        closesocket(s);
        WSACleanup();
    }
    let _ = fs::write(&result_path, &result);
    // Optionally do what ModEngine2 does for the files the game asks for: ask whether the file exists in the mod folder
    // (GetFileAttributesExW, with the mixed separators of its log: `<mod folder>\msg\...`) and open it when it does.
    if let Ok(mod_dir) = std::env::var("ASHEN_FAKE_MOD_DIR") {
        for rel in ["msg\\engus\\item_dlc2.msgbnd.dcx", "parts\\wp_a_0200.partsbnd.dcx", "menu\\win\\01_900_black.gfx"] {
            let path = format!("{}/{rel}", mod_dir.replace('\\', "/"));
            let mut info = [0u8; 36];
            let exists = unsafe { GetFileAttributesExW(wide(&path).as_ptr(), 0, &mut info) } != 0;
            if exists {
                let _ = fs::File::open(&path);
            }
        }
    }
    // Optionally keep game-style name strings in memory (an 8-byte block header, the zero-ended UTF-16 text, at an address
    // divisible by 8) so that the in-memory rename can be tested end to end; what they read afterwards goes to the result file.
    let mut pool: Vec<u64> = Vec::new();
    if std::env::var("ASHEN_FAKE_PLANT_NAMES").as_deref() == Ok("1") {
        pool = vec![0u64; 512];
        let bytes = unsafe { std::slice::from_raw_parts_mut(pool.as_mut_ptr() as *mut u8, pool.len() * 8) };
        for (i, text) in ["Shortsword", "Avelyn", "Standard Bolt", "Longsword", "Shortsword +1"].iter().enumerate() {
            let at = i * 64;
            bytes[at..at + 8].copy_from_slice(&[0x40, 0x46, 0xdc, 0x76, 0xcd, 0x01, 0x00, 0x00]);
            for (k, u) in text.encode_utf16().enumerate() {
                bytes[at + 8 + k * 2..at + 10 + k * 2].copy_from_slice(&u.to_le_bytes());
            }
        }
    }
    // Optionally stay alive a while longer (the in-game probe's first scan runs 25 s after it starts).
    if let Some(secs) = std::env::var("ASHEN_FAKE_HOLD_SECS").ok().and_then(|v| v.parse::<u32>().ok()) {
        unsafe { Sleep(secs * 1000) };
    }
    if !pool.is_empty() {
        let bytes = unsafe { std::slice::from_raw_parts(pool.as_ptr() as *const u8, pool.len() * 8) };
        let names: Vec<String> = (0..5)
            .map(|i| {
                let units: Vec<u16> = bytes[i * 64 + 8..i * 64 + 64].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
                String::from_utf16_lossy(&units)
            })
            .collect();
        result.push_str(&format!("names_after={}\n", names.join("|")));
        let _ = fs::write(&result_path, &result);
    }
    ExitCode::SUCCESS
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let stem = std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase())).unwrap_or_default();
    match stem.as_str() {
        "modengine2_launcher" => fake_me2(&args),
        "darksoulsiii" => fake_ds3(),
        _ => match args.get(1).map(String::as_str) {
            Some("probe") => probe(&args),
            _ => {
                eprintln!("usage: ashen-harness probe --dll <hook.dll> --other <file> [--block true|false]");
                ExitCode::from(64)
            }
        },
    }
}
