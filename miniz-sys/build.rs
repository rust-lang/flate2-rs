extern crate cc;

use std::env;

fn main() {
    let target = env::var("TARGET").unwrap();
    if target == "wasm32-unknown-unknown" {
        return
    }
    let mut build = cc::Build::new();
    build.file("miniz.c")
        .define("MINIZ_NO_STDIO", None)
        .define("MINIZ_NO_ARCHIVE_APIS", None)
        .define("MINIZ_NO_ARCHIVE_WRITING_APIS", None)
        .define("MINIZ_NO_TIME", None)
        .define("MINIZ_NO_ZLIB_COMPATIBLE_NAMES", None)
        .warnings(false);

    if !target.contains("darwin") && !target.contains("windows") {
        build.flag("-fvisibility=hidden");
    }

    build.compile("miniz");
    println!("cargo:root={}", env::var("OUT_DIR").unwrap());
}
