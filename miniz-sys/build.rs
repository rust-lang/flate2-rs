extern crate cc;

use std::env;

fn main() {
    cc::Build::new()
        .file("miniz.c")
        .warnings(false)
        .compile("miniz");
    println!("cargo:root={}", env::var("OUT_DIR").unwrap());
}
