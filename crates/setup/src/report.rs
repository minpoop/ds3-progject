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

    /// A longer message for the report file and the console, broken at spaces into lines of about 100 characters (a
    /// single report line is cut at 220), each starting with `indent`.
    pub fn say_wrapped(&mut self, indent: &str, text: &str) {
        for line in wrap(text, 100usize.saturating_sub(indent.len()).max(40)) {
            self.say(format!("{indent}{line}"));
        }
    }

    /// A list item for the report file and the console: `bullet` in front of the first line, spaces of the same width
    /// in front of the lines that follow.
    pub fn say_item(&mut self, bullet: &str, text: &str) {
        let pad = " ".repeat(bullet.chars().count());
        for (i, line) in wrap(text, 100usize.saturating_sub(pad.len()).max(40)).into_iter().enumerate() {
            self.say(format!("{}{line}", if i == 0 { bullet } else { &pad }));
        }
    }

    /// Like `say_wrapped`, for the report file only.
    pub fn detail_wrapped(&mut self, indent: &str, text: &str) {
        for line in wrap(text, 100usize.saturating_sub(indent.len()).max(40)) {
            self.detail(format!("{indent}{line}"));
        }
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

/// `text` broken at spaces into lines of at most `width` characters (a longer single word keeps its own line).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_messages_are_broken_at_spaces() {
        assert_eq!(wrap("one two  three four", 9), vec!["one two", "three", "four"]);
        assert_eq!(wrap("a-very-long-word-that-does-not-fit and more", 10), vec!["a-very-long-word-that-does-not-fit", "and more"]);
        assert!(wrap("   ", 10).is_empty());
    }

    #[test]
    fn wrapped_text_reaches_the_report_whole() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("r.txt");
        let mut rep = Report::create(&path);
        let long = "word ".repeat(120);
        rep.say_wrapped("  ", &long);
        rep.detail_wrapped("      ", &long);
        rep.say_item("  - ", &long);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.lines().all(|l| l.chars().count() <= 100), "{text}");
        assert_eq!(text.matches("word").count(), 360, "nothing is cut off");
    }

    #[test]
    fn list_items_hang_under_their_first_line() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("r.txt");
        let mut rep = Report::create(&path);
        rep.say_item("  - ", &format!("first {}", "word ".repeat(40)));
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.len() > 1 && lines[0].starts_with("  - first") && lines[1..].iter().all(|l| l.starts_with("    word")), "{text}");
    }
}
