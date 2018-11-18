#![feature(test)]

extern crate flate2_crc;
extern crate rand;
extern crate test;
extern crate miniz_sys;
extern crate libz_sys;

use rand::{thread_rng, RngCore};

fn flate2_crc(data: &[u8]) -> u32 {
    flate2_crc::Hardware::detect().calculate(0, data, |crc, data| {
        unsafe {
            miniz_sys::mz_crc32(crc as u64, data.as_ptr(), data.len()) as u32
        }
    })
}

fn miniz(data: &[u8]) -> u32 {
    unsafe {
        miniz_sys::mz_crc32(0, data.as_ptr(), data.len()) as u32
    }
}

fn zlib(data: &[u8]) -> u32 {
    unsafe {
        libz_sys::crc32(0, data.as_ptr(), data.len() as u32) as u32
    }
}

macro_rules! benches {
    ($($f:ident => ($small:ident, $medium:ident, $large:ident),)*) => ($(
        #[bench]
        fn $small(b: &mut test::Bencher) {
            let mut rng = thread_rng();
            let mut buf = vec![0u8; 8];
            rng.fill_bytes(&mut buf);

            b.bytes = 8;
            b.iter(|| $f(&buf));
        }

        #[bench]
        fn $medium(b: &mut test::Bencher) {
            let mut rng = thread_rng();
            let mut buf = vec![0u8; 65_000];
            rng.fill_bytes(&mut buf);

            b.bytes = 65_000;
            b.iter(|| $f(&buf));
        }

        #[bench]
        fn $large(b: &mut test::Bencher) {
            let mut rng = thread_rng();
            let mut buf = vec![0u8; 1_000_000];
            rng.fill_bytes(&mut buf);

            b.bytes = 1_000_000;
            b.iter(|| $f(&buf));
        }
    )*)
}

benches! {
    flate2_crc => (flate2_crc_8, flate2_crc_65000, flate2_crc_1000000),
    miniz => (miniz_8, miniz_65000, miniz_1000000),
    zlib => (zlib_8, zlib_65000, zlib_1000000),
}
