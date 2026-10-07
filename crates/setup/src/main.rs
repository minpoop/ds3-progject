//! ashenmarine-setup: reads the player's own Space Marine 2 install (read-only).
//!
//! ```text
//! ashenmarine-setup probe   [--sm2 "<folder>"] [--out "<folder>"]
//! ashenmarine-setup prepare [--sm2 "<folder>"] [--out "<assets folder>"]
//! ```
//!
//! `probe` is the default when no command is given (so a double-click works). Exit codes: 0 done, 2 nothing could be
//! done (the message says why), 64 the command line was not understood.
use ashen_setup::{exe_dir, prepare, probe, sm2_hint};

use std::path::PathBuf;
use std::process::ExitCode;

fn usage() {
    println!("ashenmarine-setup probe   [--sm2 \"<Space Marine 2 folder>\"] [--out \"<report folder>\"]");
    println!("  Looks at your Space Marine 2 install (read-only) and writes probe-report.txt and some preview pictures.");
    println!("ashenmarine-setup prepare [--sm2 \"<Space Marine 2 folder>\"] [--out \"<assets folder>\"]");
    println!("  Turns the Space Marine 2 weapon sounds the mod needs into plain .wav files below <assets folder>\\sounds");
    println!("  (default: the folder \"assets\" next to this program) and writes prepare-sm2\\prepare-report.txt next to it.");
    println!("  Space Marine 2 is only read. If it is not found, put its folder on the first line of sm2-folder.txt next to this program.");
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Probe,
    Prepare,
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    command: Command,
    sm2: Option<PathBuf>,
    out: Option<PathBuf>,
    help: bool,
}

/// The command line (without the program name), or what is wrong with it.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut args = args.into_iter().peekable();
    let mut parsed = Args { command: Command::Probe, sm2: None, out: None, help: false };
    if args.peek().is_some_and(|a| !a.starts_with('-')) {
        parsed.command = match args.next().unwrap().as_str() {
            "probe" => Command::Probe,
            "prepare" => Command::Prepare,
            other => return Err(format!("unknown command {other:?}")),
        };
    }
    while let Some(a) = args.next() {
        match a.as_str() {
            "--sm2" => parsed.sm2 = Some(args.next().map(PathBuf::from).ok_or("--sm2 needs the Space Marine 2 folder after it")?),
            "--out" => parsed.out = Some(args.next().map(PathBuf::from).ok_or("--out needs a folder after it")?),
            "-h" | "--help" => parsed.help = true,
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(parsed)
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(why) => {
            eprintln!("{why}");
            usage();
            return ExitCode::from(64);
        }
    };
    if args.help {
        usage();
        return ExitCode::SUCCESS;
    }
    let dir = exe_dir();
    // optional hint for a game that is not in a normal Steam library: first line of sm2-folder.txt next to the exe
    let sm2 = args.sm2.or_else(|| sm2_hint(&dir));
    let done = match args.command {
        Command::Probe => probe::run(&probe::Opts { sm2, out: args.out.unwrap_or_else(|| dir.join("probe-out")) }),
        Command::Prepare => prepare::run(&prepare::Opts { sm2, out: args.out.unwrap_or_else(|| dir.join("assets")) }).ok(),
    };
    if done {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn no_command_means_probe_so_a_double_click_works() {
        let a = parse(&[]).unwrap();
        assert_eq!((a.command, a.sm2, a.out, a.help), (Command::Probe, None, None, false));
        assert_eq!(parse(&["--out", "x"]).unwrap().command, Command::Probe);
        assert_eq!(parse(&["probe"]).unwrap().command, Command::Probe);
    }

    #[test]
    fn prepare_takes_the_game_folder_and_the_assets_folder() {
        let a = parse(&["prepare", "--sm2", "D:\\Games\\Space Marine 2", "--out", "assets"]).unwrap();
        assert_eq!(a, Args { command: Command::Prepare, sm2: Some(PathBuf::from("D:\\Games\\Space Marine 2")), out: Some(PathBuf::from("assets")), help: false });
        assert_eq!(parse(&["--out", "a", "prepare"]), Err("unknown option \"prepare\"".to_string()), "the command comes first");
        assert!(parse(&["prepare", "--help"]).unwrap().help);
    }

    #[test]
    fn bad_arguments_are_named() {
        assert_eq!(parse(&["extract"]), Err("unknown command \"extract\"".to_string()));
        assert_eq!(parse(&["prepare", "--sm3", "x"]), Err("unknown option \"--sm3\"".to_string()));
        assert!(parse(&["prepare", "--sm2"]).unwrap_err().contains("--sm2 needs"));
        assert!(parse(&["probe", "--out"]).unwrap_err().contains("--out needs"));
    }
}
