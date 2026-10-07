//! A report that is written to disk line by line (so a crash never loses what was already found) and echoed to the
//! console in a shorter form.
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

pub struct Report {
    file: Option<File>,
    started: Instant,
    pub lines: usize,
}

impl Report {
    pub fn create(path: &Path) -> Report {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Report { file: File::create(path).ok(), started: Instant::now(), lines: 0 }
    }

    fn write(&mut self, text: &str) {
        self.lines += 1;
        if let Some(f) = &mut self.file {
            let _ = writeln!(f, "{text}");
            let _ = f.flush();
        }
    }

    /// A line for the report file only.
    pub fn detail(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        let text = if text.chars().count() > 220 { format!("{}...", text.chars().take(217).collect::<String>()) } else { text.to_string() };
        self.write(&text);
    }

    /// A line for the report file and the console.
    pub fn say(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        println!("{text}");
        self.detail(text);
    }

    /// Console progress that is not worth keeping in the file.
    pub fn progress(&self, text: &str) {
        println!("{text}");
    }

    pub fn section(&mut self, title: &str) {
        self.detail("");
        self.detail(format!("==== {title} ({:.1}s) ====", self.started.elapsed().as_secs_f32()));
        println!("\n{title} ...");
    }

    pub fn elapsed(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }
}

/// A one-line description of a panic payload, for "this step crashed" lines.
pub fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
