//! Read-only access to the player's own Space Marine 2 install: paks (zip), textures, sound banks.
//! Nothing in this crate writes to the install, and nothing in it contains game data.
pub mod bnk;
pub mod hirc;
pub mod pak;
pub mod texture;
pub mod wem;
pub mod wwvorbis;
