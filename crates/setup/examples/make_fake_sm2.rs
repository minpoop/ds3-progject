//! Builds the synthetic Space Marine 2 install used by the tests:  make_fake_sm2 <folder> [--weapons]
//! No game data is in it: weapon files, textures and text are made up, and the weapon sound bank holds a few events with
//! the real names of design/sheets/sounds.json whose sounds are plain generated tones (see `common::weapon_sounds`).
//! Both commands of the setup program can be tried on it:
//!   ashenmarine-setup probe   --sm2 <folder> --out <report folder>
//!   ashenmarine-setup prepare --sm2 <folder> --out <assets folder>
//! With `--weapons` it also holds made-up templates of the chainsword and the bolt pistol (one square each) and the picture their
//! material names, so that `ashenmarine-setup ds3-models` can be tried together with the synthetic Dark Souls III.
#[path = "../tests/common/mod.rs"]
mod common;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: make_fake_sm2 <folder> [--weapons]");
    common::fake_install(std::path::Path::new(&dir));
    if args.next().as_deref() == Some("--weapons") {
        common::add_made_up_weapons(std::path::Path::new(&dir));
    }
    println!("fake install written to {dir}");
}
