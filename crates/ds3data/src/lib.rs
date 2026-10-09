//! Ashen Marine's reader for Dark Souls III's own data files, and the patcher that uses it.
//!
//! Everything here is pure Rust that runs and is tested on any OS, and **read-only**: game files are opened for reading
//! and never created, changed or deleted. What the patcher produces is returned as bytes; the caller decides where to
//! write them (and never into the game's folder).
//!
//! This is an independent implementation. The file layouts it reads and writes are the ones documented by the Souls
//! modding community (the `BHD5`/`BDT` archive pair with its RSA-protected header and path hashes, `DCX` compression,
//! `BND4` containers and `FMG` text tables); only those facts (field order, sizes, constants) were used, no code.
//!
//! * [`hash`] - the path hash the archives use instead of file names
//! * [`keys`] - RSA public keys found as PEM text in the player's own `DarkSoulsIII.exe`, fingerprints
//! * [`rsa`] - the raw RSA operation that unlocks an archive header
//! * [`bhd5`], [`archive`], [`install`] - the archive header, one archive, and the whole install
//! * [`dcx`], [`bnd4`], [`fmg`] - the containers inside the archives; `bnd4::replace_file` changes one file of a BND4 and
//!   nothing else
//! * [`msgpatch`] - changes item names and descriptions in `item.msgbnd.dcx`, failing closed
//! * [`scan`] - recognises archive keys and decrypted tables of contents in raw bytes (the running game's memory, the program
//!   file); the test kit's collector in the hook uses it, this crate reads nothing from a process itself
#![forbid(unsafe_code)]

pub mod archive;
pub mod bhd5;
pub mod bnd4;
pub mod dcx;
pub mod fmg;
pub mod hash;
pub mod install;
pub mod keys;
pub mod msgpatch;
pub mod rsa;
pub mod scan;
mod util;

pub use util::{hex, sha256_hex, snippet};

/// Builders of synthetic keys, archives and files for tests (no game data).
#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Damaged and hostile input for every parser.
#[cfg(test)]
mod robustness;
