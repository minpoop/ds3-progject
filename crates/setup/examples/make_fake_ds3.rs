//! Builds the synthetic Dark Souls III install used by the tests:  make_fake_ds3 <folder> [options]
//! No game data is in it: the program file is junk with the two throwaway test keys written into it as text, the archives
//! (`Data0`, `Data1`, `DLC1`) are made with those keys and hold made-up item text, menu text, regulation and weapon model
//! containers. Both Dark Souls III commands of the setup program can be tried on it:
//!   ashenmarine-setup ds3-probe   --ds3 <folder> --out <report folder>
//!   ashenmarine-setup ds3-prepare --ds3 <folder> --mod <mod folder> --out <report folder>
//!
//! Options (to make the situations the tests and the Wine test need):
//!   --variant good|base-only|dlc1-renamed|wrong-name|no-item|french-only|damaged|unpatchable|unexpected-path|unexpected-wrong-name
//!                                what the item text is like (default: good = item_dlc2 and item_dlc1, as the real game has
//!                                them; base-only = only item.msgbnd.dcx); the last two store it under a name nobody expects
//!                                (a look at what the files contain finds it, but the name the game asks for stays unknown)
//!   --no-exe-keys                the program file holds no key as text (give the keys with  --keys, see --write-keys)
//!   --write-keys <file>          also write the two test keys to a PEM file
//!   --save-headers <folder>      also write the plain table of contents of every archive there, as `<name>.bin` - what the
//!                                test kit's collector saves from the running game (`ashenmarine/cache/bhd5`)
//!   --plain-data0                Data0.bhd is a plain (not encrypted) table of contents, as in the real game
//!   --broken                     add a .bhd without a .bdt and one that no key opens
use ashen_ds3data::testing::install::{build, FakeOptions, ItemMsg};
use ashen_ds3data::testing::keys::test_key;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let folder = args.next().expect("usage: make_fake_ds3 <folder> [--variant NAME] [--no-exe-keys] [--write-keys FILE] [--broken]");
    let mut opts = FakeOptions::default();
    let mut write_keys: Option<PathBuf> = None;
    let mut save_headers: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--variant" => {
                opts.item_msg = match args.next().expect("--variant needs a name").as_str() {
                    "good" => ItemMsg::Good,
                    "base-only" => ItemMsg::BaseOnly,
                    "dlc1-renamed" => ItemMsg::Dlc1Renamed,
                    "wrong-name" => ItemMsg::WrongName,
                    "no-item" => ItemMsg::Missing,
                    "french-only" => ItemMsg::FrenchOnly,
                    "damaged" => ItemMsg::DamagedDcx,
                    "unpatchable" => ItemMsg::UnpatchableLayout,
                    "unexpected-path" => ItemMsg::UnexpectedPath,
                    "unexpected-wrong-name" => ItemMsg::UnexpectedPathWrongName,
                    other => panic!("unknown variant {other}"),
                }
            }
            "--no-exe-keys" => opts.exe_keys.clear(),
            "--write-keys" => write_keys = Some(PathBuf::from(args.next().expect("--write-keys needs a file"))),
            "--save-headers" => save_headers = Some(PathBuf::from(args.next().expect("--save-headers needs a folder"))),
            "--plain-data0" => opts.plain_data0 = true,
            "--broken" => opts.broken_archives = true,
            other => panic!("unknown option {other}"),
        }
    }
    let fake = build(std::path::Path::new(&folder), &opts);
    if let Some(path) = write_keys {
        std::fs::write(&path, format!("{}{}", test_key(0).pem(), test_key(1).pem())).expect("write the key file");
    }
    if let Some(dir) = save_headers {
        std::fs::create_dir_all(&dir).expect("create the header folder");
        for (name, key) in [("Data0", 0usize), ("Data1", 1), ("DLC1", 0)] {
            let bhd = std::fs::read(fake.game.join(format!("{name}.bhd"))).expect("read the .bhd");
            let mut plain = ashen_ds3data::rsa::decrypt_header(&test_key(key).public, &bhd).expect("decrypt the header");
            let declared = i32::from_le_bytes(plain[0x0C..0x10].try_into().expect("four bytes")) as usize;
            plain.truncate(declared);
            std::fs::write(dir.join(format!("{name}.bin")), plain).expect("write the plain header");
        }
    }
    println!("fake Dark Souls III install written to {}", fake.root.display());
}
