//! ashenmarine-setup: reads the player's own Space Marine 2 and Dark Souls III installs (read-only).
//!
//! ```text
//! ashenmarine-setup probe          [--sm2 "<folder>"] [--out "<folder>"]
//! ashenmarine-setup prepare        [--sm2 "<folder>"] [--out "<assets folder>"]
//! ashenmarine-setup sm2-mesh-probe [--sm2 "<folder>"] [--out "<folder>"]
//! ashenmarine-setup ds3-probe      [--ds3 "<folder>"] [--keys "<pem file>"] [--out "<report folder>"]
//! ashenmarine-setup ds3-prepare    [--ds3 "<folder>"] [--keys "<pem file>"] [--mod "<mod folder>"] [--out "<report folder>"]
//! ```
//!
//! `probe` is the default when no command is given (so a double-click works). Exit codes: 0 done, 2 nothing could be
//! done (the message says why), 64 the command line was not understood.
use ashen_setup::{ds3, ds3_hint, exe_dir, meshprobe, prepare, probe, sm2_hint};

use std::path::PathBuf;
use std::process::ExitCode;

fn usage() {
    println!("ashenmarine-setup probe   [--sm2 \"<Space Marine 2 folder>\"] [--out \"<report folder>\"]");
    println!("  Looks at your Space Marine 2 install (read-only) and writes probe-report.txt and some preview pictures.");
    println!("ashenmarine-setup prepare [--sm2 \"<Space Marine 2 folder>\"] [--out \"<assets folder>\"]");
    println!("  Turns the Space Marine 2 weapon sounds the mod needs into plain .wav files below <assets folder>\\sounds");
    println!("  (default: the folder \"assets\" next to this program) and writes prepare-sm2\\prepare-report.txt next to it.");
    println!("  Space Marine 2 is only read. If it is not found, put its folder on the first line of sm2-folder.txt next to this program.");
    println!("ashenmarine-setup sm2-mesh-probe [--sm2 \"<Space Marine 2 folder>\"] [--out \"<report folder>\"]");
    println!("  Looks at the chainsword and bolt pistol model files of your Space Marine 2 (read-only) and writes mesh-report.txt:");
    println!("  numbers and names about the files, no copies of them (default folder: probe-sm2-mesh next to this program).");
    println!("ashenmarine-setup ds3-probe   [--ds3 \"<Dark Souls III folder>\"] [--keys \"<key file>\"] [--out \"<report folder>\"]");
    println!("  Looks into your Dark Souls III archives (read-only) and writes ds3-prepare\\ds3-report.txt next to this program.");
    println!("ashenmarine-setup ds3-prepare [--ds3 \"<Dark Souls III folder>\"] [--keys \"<key file>\"] [--mod \"<mod folder>\"] [--out \"<report folder>\"]");
    println!("  Reads the game's item text from its archives (read-only) and writes a copy with the test weapons' new names to");
    println!("  <mod folder>\\msg\\ENGLISH\\item.msgbnd.dcx (default: the folder \"mod\" next to this program), only if every check passes.");
    println!("  Dark Souls III is only read. If it is not found, put its folder on the first line of game-folder.txt next to this program.");
    println!("  --keys names a text file with the archive keys (-----BEGIN RSA PUBLIC KEY-----), in case the game's program file has none.");
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Command {
    Probe,
    Prepare,
    MeshProbe,
    Ds3Probe,
    Ds3Prepare,
}

impl Command {
    fn name(self) -> &'static str {
        match self {
            Command::Probe => "probe",
            Command::Prepare => "prepare",
            Command::MeshProbe => "sm2-mesh-probe",
            Command::Ds3Probe => "ds3-probe",
            Command::Ds3Prepare => "ds3-prepare",
        }
    }

    fn is_ds3(self) -> bool {
        matches!(self, Command::Ds3Probe | Command::Ds3Prepare)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    command: Command,
    sm2: Option<PathBuf>,
    ds3: Option<PathBuf>,
    keys: Option<PathBuf>,
    mod_dir: Option<PathBuf>,
    out: Option<PathBuf>,
    help: bool,
}

/// The command line (without the program name), or what is wrong with it.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut args = args.into_iter().peekable();
    let mut parsed = Args { command: Command::Probe, sm2: None, ds3: None, keys: None, mod_dir: None, out: None, help: false };
    if args.peek().is_some_and(|a| !a.starts_with('-')) {
        parsed.command = match args.next().unwrap().as_str() {
            "probe" => Command::Probe,
            "prepare" => Command::Prepare,
            "sm2-mesh-probe" => Command::MeshProbe,
            "ds3-probe" => Command::Ds3Probe,
            "ds3-prepare" => Command::Ds3Prepare,
            other => return Err(format!("unknown command {other:?}")),
        };
    }
    while let Some(a) = args.next() {
        match a.as_str() {
            "--sm2" => parsed.sm2 = Some(args.next().map(PathBuf::from).ok_or("--sm2 needs the Space Marine 2 folder after it")?),
            "--ds3" => parsed.ds3 = Some(args.next().map(PathBuf::from).ok_or("--ds3 needs the Dark Souls III folder after it")?),
            "--keys" => parsed.keys = Some(args.next().map(PathBuf::from).ok_or("--keys needs the key file after it")?),
            "--mod" => parsed.mod_dir = Some(args.next().map(PathBuf::from).ok_or("--mod needs a folder after it")?),
            "--out" => parsed.out = Some(args.next().map(PathBuf::from).ok_or("--out needs a folder after it")?),
            "-h" | "--help" => parsed.help = true,
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    // an option that does not belong to the command is a mistake worth naming
    let c = parsed.command;
    if c.is_ds3() && parsed.sm2.is_some() {
        return Err(format!("--sm2 is for Space Marine 2, not for {}", c.name()));
    }
    if !c.is_ds3() && (parsed.ds3.is_some() || parsed.keys.is_some() || parsed.mod_dir.is_some()) {
        let which = if parsed.ds3.is_some() { "--ds3" } else if parsed.keys.is_some() { "--keys" } else { "--mod" };
        return Err(format!("{which} is for the Dark Souls III commands (ds3-probe, ds3-prepare), not for {}", c.name()));
    }
    if c == Command::Ds3Probe && parsed.mod_dir.is_some() {
        return Err("--mod is only used by ds3-prepare".to_string());
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
    let done = match args.command {
        Command::Probe | Command::Prepare | Command::MeshProbe => {
            // optional hint for a game that is not in a normal Steam library: first line of sm2-folder.txt next to the exe
            let sm2 = args.sm2.or_else(|| sm2_hint(&dir));
            match args.command {
                Command::Probe => probe::run(&probe::Opts { sm2, out: args.out.unwrap_or_else(|| dir.join("probe-out")) }),
                Command::Prepare => prepare::run(&prepare::Opts { sm2, out: args.out.unwrap_or_else(|| dir.join("assets")), normalize: true, ab: true }).ok(),
                _ => meshprobe::run(&meshprobe::Opts { sm2, out: args.out.unwrap_or_else(|| dir.join("probe-sm2-mesh")) }),
            }
        }
        Command::Ds3Probe | Command::Ds3Prepare => {
            // the same idea for Dark Souls III: first line of game-folder.txt next to the exe (the launcher reads it, too)
            let ds3 = args.ds3.or_else(|| ds3_hint(&dir));
            let out = args.out.unwrap_or_else(|| dir.join("ds3-prepare"));
            if args.command == Command::Ds3Probe {
                ds3::probe(&ds3::ProbeOpts { ds3, keys: args.keys, data: dir, out })
            } else {
                let mod_dir = args.mod_dir.unwrap_or_else(|| dir.join("mod"));
                ds3::prepare(&ds3::PrepareOpts { ds3, keys: args.keys, data: dir, out, mod_dir }).ok()
            }
        }
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

    fn args(command: Command) -> Args {
        Args { command, sm2: None, ds3: None, keys: None, mod_dir: None, out: None, help: false }
    }

    #[test]
    fn no_command_means_probe_so_a_double_click_works() {
        let a = parse(&[]).unwrap();
        assert_eq!(a, args(Command::Probe));
        assert_eq!(parse(&["--out", "x"]).unwrap().command, Command::Probe);
        assert_eq!(parse(&["probe"]).unwrap().command, Command::Probe);
    }

    #[test]
    fn prepare_takes_the_game_folder_and_the_assets_folder() {
        let a = parse(&["prepare", "--sm2", "D:\\Games\\Space Marine 2", "--out", "assets"]).unwrap();
        assert_eq!(a, Args { sm2: Some(PathBuf::from("D:\\Games\\Space Marine 2")), out: Some(PathBuf::from("assets")), ..args(Command::Prepare) });
        assert_eq!(parse(&["--out", "a", "prepare"]), Err("unknown option \"prepare\"".to_string()), "the command comes first");
        assert!(parse(&["prepare", "--help"]).unwrap().help);
    }

    #[test]
    fn the_mesh_probe_is_a_command_of_its_own() {
        let a = parse(&["sm2-mesh-probe", "--out", "x"]).unwrap();
        assert_eq!((a.command, a.out), (Command::MeshProbe, Some(PathBuf::from("x"))));
        assert_eq!(parse(&["sm2-mesh-probe", "--sm2", "y"]).unwrap().sm2, Some(PathBuf::from("y")));
        assert!(parse(&["sm2-mesh-probe", "--ds3", "y"]).unwrap_err().contains("--ds3 is for the Dark Souls III commands"));
    }

    #[test]
    fn the_dark_souls_commands_take_their_own_options() {
        let a = parse(&["ds3-probe", "--ds3", "D:\\Games\\DARK SOULS III", "--keys", "keys.pem", "--out", "report"]).unwrap();
        assert_eq!(
            a,
            Args { ds3: Some(PathBuf::from("D:\\Games\\DARK SOULS III")), keys: Some(PathBuf::from("keys.pem")), out: Some(PathBuf::from("report")), ..args(Command::Ds3Probe) }
        );
        let b = parse(&["ds3-prepare", "--mod", "mymod", "--ds3", "x", "--keys", "k.pem"]).unwrap();
        assert_eq!(b, Args { ds3: Some(PathBuf::from("x")), keys: Some(PathBuf::from("k.pem")), mod_dir: Some(PathBuf::from("mymod")), ..args(Command::Ds3Prepare) });
        assert_eq!(parse(&["ds3-prepare"]).unwrap(), args(Command::Ds3Prepare));
        assert!(parse(&["ds3-probe", "--help"]).unwrap().help);
    }

    #[test]
    fn options_for_the_other_game_are_named_as_mistakes() {
        assert_eq!(parse(&["ds3-probe", "--sm2", "x"]), Err("--sm2 is for Space Marine 2, not for ds3-probe".to_string()));
        assert_eq!(parse(&["ds3-prepare", "--sm2", "x"]), Err("--sm2 is for Space Marine 2, not for ds3-prepare".to_string()));
        assert!(parse(&["prepare", "--ds3", "x"]).unwrap_err().contains("--ds3 is for the Dark Souls III commands"));
        assert!(parse(&["probe", "--keys", "x"]).unwrap_err().contains("--keys is for the Dark Souls III commands"));
        assert!(parse(&["probe", "--mod", "x"]).unwrap_err().contains("--mod is for the Dark Souls III commands"));
        assert_eq!(parse(&["ds3-probe", "--mod", "x"]), Err("--mod is only used by ds3-prepare".to_string()));
    }

    #[test]
    fn bad_arguments_are_named() {
        assert_eq!(parse(&["extract"]), Err("unknown command \"extract\"".to_string()));
        assert_eq!(parse(&["prepare", "--sm3", "x"]), Err("unknown option \"--sm3\"".to_string()));
        assert!(parse(&["prepare", "--sm2"]).unwrap_err().contains("--sm2 needs"));
        assert!(parse(&["probe", "--out"]).unwrap_err().contains("--out needs"));
        assert!(parse(&["ds3-prepare", "--ds3"]).unwrap_err().contains("--ds3 needs"));
        assert!(parse(&["ds3-prepare", "--keys"]).unwrap_err().contains("--keys needs"));
        assert!(parse(&["ds3-prepare", "--mod"]).unwrap_err().contains("--mod needs"));
        assert_eq!(parse(&["ds3-prepare", "extra"]), Err("unknown option \"extra\"".to_string()));
    }
}
