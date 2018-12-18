extern crate ctest;

fn main() {
    let mut cfg = ctest::TestGenerator::new();
    cfg.header("miniz.h")
        .include(concat!(env!("CARGO_MANIFEST_DIR"), "/../miniz-sys"))
        .type_name(|s, _, _| {
            if s == "mz_internal_state" {
                "struct mz_internal_state".to_string()
            } else {
                s.to_string()
            }
        })
        .skip_field(|s, f| {
            // We switched this from `*mut c_char` to `*const c_char`
            s == "mz_stream" && f == "msg"
        })
        .skip_signededness(|s| s.ends_with("_func"));

    cfg.generate("../miniz-sys/lib.rs", "all.rs");
}
