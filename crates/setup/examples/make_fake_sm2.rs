//! Builds the synthetic Space Marine 2 install used by the tests:  make_fake_sm2 <folder>
//! No game data is in it: weapon files, textures and text are made up, and the weapon sound bank holds a few events with
//! the real names of design/sheets/sounds.json whose sounds are plain generated tones (see `common::weapon_sounds`).
//! Both commands of the setup program can be tried on it:
//!   ashenmarine-setup probe   --sm2 <folder> --out <report folder>
//!   ashenmarine-setup prepare --sm2 <folder> --out <assets folder>
#[path = "../tests/common/mod.rs"]
mod common;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: make_fake_sm2 <folder>");
    common::fake_install(std::path::Path::new(&dir));
    println!("fake install written to {dir}");
}
