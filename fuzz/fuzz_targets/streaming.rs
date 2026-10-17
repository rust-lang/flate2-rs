#![no_main]
#[path = "../streaming.rs"]
mod streaming;
libfuzzer_sys::fuzz_target!(|data: &[u8]| streaming::exercise(data));
