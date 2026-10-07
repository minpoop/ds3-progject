//! Builds the synthetic Space Marine 2 install used by the tests:  make_fake_sm2 <folder>
#[path = "../tests/common/mod.rs"]
mod common;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: make_fake_sm2 <folder>");
    common::fake_install(std::path::Path::new(&dir));
    println!("fake install written to {dir}");
}
