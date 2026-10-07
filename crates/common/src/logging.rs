//! Tiny append-only text logger plus UTC time formatting (no external time crate).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 -> (year, month, day), proleptic Gregorian (Howard Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day, hour, minute, second, millis) in UTC for a unix time.
pub fn utc_from_unix(secs: u64, millis: u32) -> (i64, u32, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let (y, m, d) = civil_from_days(days);
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60, millis)
}

fn now() -> (u64, u32) {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (d.as_secs(), d.subsec_millis())
}

/// "2026-10-07 03:21:45.123" (UTC)
pub fn timestamp() -> String {
    let (s, ms) = now();
    let (y, mo, d, h, mi, se, ms) = utc_from_unix(s, ms);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{se:02}.{ms:03}")
}

/// "20261007-032145" (UTC), good for folder names.
pub fn file_tag() -> String {
    let (s, ms) = now();
    let (y, mo, d, h, mi, se, _) = utc_from_unix(s, ms);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{se:02}")
}

pub struct Logger {
    tag: &'static str,
    file: Mutex<Option<File>>,
}

impl Logger {
    /// Open (append) a log file, creating its folder. A log over 2 MB is started afresh.
    /// Never fails: a logger that cannot open its file just discards lines.
    pub fn open(path: &Path, tag: &'static str) -> Logger {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::metadata(path).map(|m| m.len() > 2_000_000).unwrap_or(false) {
            let _ = fs::remove_file(path);
        }
        let file = OpenOptions::new().create(true).append(true).open(path).ok();
        Logger { tag, file: Mutex::new(file) }
    }

    pub fn log(&self, msg: &str) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = writeln!(f, "[{}] [{}] {}", timestamp(), self.tag, msg);
                let _ = f.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // leap day
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn utc_formatting() {
        // 2026-10-07 03:21:45 UTC
        let (y, mo, d, h, mi, s, ms) = utc_from_unix(1_791_343_305, 123);
        assert_eq!((y, mo, d, h, mi, s, ms), (2026, 10, 7, 3, 21, 45, 123));
    }

    #[test]
    fn logger_writes_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/x.log");
        let l = Logger::open(&p, "test");
        l.log("hello");
        l.log("world");
        let text = fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("[test] hello"));
    }
}
