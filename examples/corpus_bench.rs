//! Compression benchmark for source, executable bytes, and low-entropy input.
//! Pass a directory containing `rust-source.bin` and `rg-binary.bin` snapshots.
//! Run with `cargo run --release --no-default-features --features fdeflate
//! --example corpus_bench -- <directory>`. Output is CSV; allocations are reused.
#![forbid(unsafe_code)]
use flate2::{Compress, Compression, FlushCompress, Status};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
fn main() {
    let folder = std::env::args().nth(1).unwrap();
    println!("pattern,input_bytes,level,seconds,compressed_bytes");
    for pattern in ["source", "binary", "low16", "skewed"] {
        let input = match pattern {
            "source" => std::fs::read(format!("{folder}/rust-source.bin")).unwrap(),
            "binary" => std::fs::read(format!("{folder}/rg-binary.bin")).unwrap(),
            _ => {
                let mut r = 12345678u32;
                (0..1048576)
                    .map(|_| {
                        r ^= r << 13;
                        r ^= r >> 17;
                        r ^= r << 5;
                        if pattern == "low16" {
                            (r % 16) as u8
                        } else {
                            match r % 100 {
                                0 => r as u8,
                                1..=2 => (r % 32) as u8,
                                11..=50 => (r % 16) as u8,
                                51..=80 => (r % 8) as u8,
                                _ => 0,
                            }
                        }
                    })
                    .collect()
            }
        };
        for level in [1, 4, 6, 9] {
            let mut encoder = Compress::new(Compression::new(level), true);
            let mut output = vec![0; 32768];
            let mut sizes = 0;
            let mut operation = || {
                encoder.reset();
                loop {
                    let status = encoder
                        .compress(
                            &input[encoder.total_in() as usize..],
                            &mut output,
                            FlushCompress::Finish,
                        )
                        .unwrap();
                    black_box(&output);
                    if status == Status::StreamEnd {
                        break;
                    }
                }
                sizes = encoder.total_out();
            };
            operation();
            let mut times = [0.; 5];
            for t in &mut times {
                let start = Instant::now();
                let mut n = 0;
                while start.elapsed() < Duration::from_millis(50) {
                    operation();
                    n += 1;
                }
                *t = start.elapsed().as_secs_f64() / n as f64;
            }
            times.sort_by(f64::total_cmp);
            println!("{pattern},{},{level},{:.9},{sizes}", input.len(), times[2]);
        }
    }
}
