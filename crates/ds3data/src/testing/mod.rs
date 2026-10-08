//! Builders for synthetic data, for tests only (compiled in tests and with the `testing` feature): test RSA keys, archives,
//! DCX/BND4/FMG files and whole fake installs. No game data is in here; every layout comes from the facts documented in
//! the other modules of this crate.
pub mod archive;
pub mod bnd4;
pub mod items;
pub mod keys;
