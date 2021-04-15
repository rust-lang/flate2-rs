#![no_main]
use libfuzzer_sys::fuzz_target;
use flate2::write::GzEncoder;
use flate2::Compression;
use flate2::read::GzDecoder;
use std::io::prelude::*;


fuzz_target!(|data: &[u8]| {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    let result = encoder.finish().unwrap();
    let mut r = GzDecoder::new(&result[..]);
    let mut ret = Vec::new();
    r.read_to_end(&mut ret).unwrap();
    assert!(ret == data);
});
