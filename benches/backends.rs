//! Throughput of whichever backend flate2 is built with, on a corpus of
//! files such as Silesia
//! (<https://sun.aei.polsl.pl/~sdeor/corpus/silesia.zip>, unzipped). Build
//! with one backend at a time:
//!
//! ```text
//! cargo bench --bench backends -- <corpus dir> [large|small]
//! cargo bench --bench backends --no-default-features --features zlib-rs -- <corpus dir>
//! ```
//!
//! `large` compresses and decompresses each whole file, and reports MB/s of
//! uncompressed data. `small` cuts the files into pieces of 64 B to 64 KiB
//! and reports the time per stream, with a fresh `Compress`/`Decompress` for
//! each piece or one reused with `reset`. Every case reports its best of a
//! few rounds.
//!
//! The decompression cases decode streams compressed by miniz_oxide at
//! level 6, so that every backend decodes the same bytes.

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use std::hint::black_box;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn main() {
    const USAGE: &str = "usage: cargo bench --bench backends -- <corpus dir> [large|small]";
    // `cargo bench` passes `--bench`.
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--bench")
        .collect();
    let Some(dir) = args.first() else {
        // Nothing to run, so a plain `cargo bench` succeeds.
        eprintln!("{}", USAGE);
        return;
    };
    let (run_large, run_small) = match args.get(1).map(String::as_str) {
        None => (true, true),
        Some("large") => (true, false),
        Some("small") => (false, true),
        Some(_) => {
            eprintln!("{}", USAGE);
            std::process::exit(2);
        }
    };
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("corpus dir")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let files: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    assert!(!files.is_empty(), "no files in {}", dir);

    if run_large {
        large(&files);
    }
    if run_small {
        small(&files);
    }
}

/// The shortest time `f` takes in `rounds` runs, and what it returned.
fn best<T>(rounds: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best = None;
    for _ in 0..rounds {
        let t = Instant::now();
        let out = black_box(f());
        let elapsed = t.elapsed();
        if best.as_ref().map_or(true, |(b, _)| elapsed < *b) {
            best = Some((elapsed, out));
        }
    }
    best.unwrap()
}

fn deflate_with_miniz(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec(data, 6)
}

/// Compresses all of `input` into `out` in one stream, with a fresh `c`.
fn compress_vec(c: &mut Compress, input: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(input.len() + input.len() / 100 + 1024);
    loop {
        let st = c
            .compress_vec(&input[c.total_in() as usize..], out, FlushCompress::Finish)
            .unwrap();
        if st == Status::StreamEnd {
            return;
        }
        out.reserve(out.capacity().max(1024));
    }
}

fn large(files: &[Vec<u8>]) {
    const ROUNDS: usize = 3;
    let total: usize = files.iter().map(Vec::len).sum();
    let deflated: Vec<Vec<u8>> = files.iter().map(|f| deflate_with_miniz(f)).collect();
    // An output buffer big enough for any file, allocated and touched once, so
    // that the timed rounds don't measure page faults.
    let largest = files.iter().map(Vec::len).max().unwrap();
    let mut out = vec![0; largest + largest / 100 + 1024];

    println!("{total} bytes in {} files", files.len());
    println!("{:<48} {:>8} {:>12}", "case", "MB/s", "compressed");
    let report = |case: &str, t: Duration, size: Option<usize>| {
        let mbs = total as f64 / t.as_secs_f64() / 1e6;
        let size = size.map_or("-".to_string(), |s| s.to_string());
        println!("{case:<48} {mbs:>8.1} {size:>12}");
    };

    for level in [1, 6, 9] {
        let (t, size) = best(ROUNDS, || {
            files
                .iter()
                .map(|f| {
                    compress_vec(
                        &mut Compress::new(Compression::new(level), false),
                        f,
                        &mut out,
                    );
                    out.len()
                })
                .sum::<usize>()
        });
        report(&format!("compress level {level}, Compress"), t, Some(size));
    }

    let (t, size) = best(ROUNDS, || {
        files
            .iter()
            .map(|f| {
                let mut w = flate2::write::DeflateEncoder::new(Vec::new(), Compression::new(6));
                for chunk in f.chunks(64 << 10) {
                    w.write_all(chunk).unwrap();
                }
                w.finish().unwrap().len()
            })
            .sum::<usize>()
    });
    report(
        "compress level 6, write::DeflateEncoder, 64 KiB",
        t,
        Some(size),
    );

    let (t, ()) = best(ROUNDS, || {
        for (f, z) in files.iter().zip(&deflated) {
            let mut d = Decompress::new(false);
            out.clear();
            let st = d.decompress_vec(z, &mut out, FlushDecompress::Finish);
            assert_eq!(st.unwrap(), Status::StreamEnd);
            assert_eq!(out.len(), f.len());
        }
    });
    report("decompress, one decompress_vec", t, None);

    let (t, ()) = best(ROUNDS, || {
        let mut buf = vec![0; 64 << 10];
        for (f, z) in files.iter().zip(&deflated) {
            let mut r = flate2::read::DeflateDecoder::new(&z[..]);
            let mut n = 0;
            loop {
                match r.read(&mut buf).unwrap() {
                    0 => break,
                    k => n += k,
                }
            }
            assert_eq!(n, f.len());
        }
    });
    report("decompress, read::DeflateDecoder, 64 KiB", t, None);
}

fn small(files: &[Vec<u8>]) {
    const ROUNDS: usize = 5;
    /// Uncompressed bytes per size.
    const BUDGET: usize = 16 << 20;
    let level = Compression::new(6);

    println!();
    println!(
        "{:>6} {:<18} {:>10} {:>8} {:>12}",
        "size", "case", "ns/stream", "MB/s", "compressed"
    );
    for size in [64, 256, 1024, 4096, 16 << 10, 64 << 10] {
        // The same number of pieces from each file, spread across it.
        let per_file = (BUDGET / size / files.len()).max(1);
        let pieces: Vec<&[u8]> = files
            .iter()
            .flat_map(|f| {
                let step = (f.len() / size / per_file).max(1);
                f.chunks_exact(size).step_by(step).take(per_file)
            })
            .collect();
        let deflated: Vec<Vec<u8>> = pieces.iter().map(|p| deflate_with_miniz(p)).collect();
        let report = |case: &str, t: Duration, size_out: Option<usize>| {
            let ns = t.as_nanos() as f64 / pieces.len() as f64;
            let mbs = (pieces.len() * size) as f64 / t.as_secs_f64() / 1e6;
            let size_out = size_out.map_or("-".to_string(), |s| s.to_string());
            println!("{size:>6} {case:<18} {ns:>10.0} {mbs:>8.1} {size_out:>12}");
        };

        let mut out = vec![0; size + size / 10 + 64];
        let mut compress = |c: &mut Compress, p: &[u8]| {
            let st = c.compress(p, &mut out, FlushCompress::Finish);
            assert_eq!(st.unwrap(), Status::StreamEnd);
            c.total_out() as usize
        };
        let (t, n) = best(ROUNDS, || {
            pieces
                .iter()
                .map(|p| compress(&mut black_box(Compress::new(level, false)), p))
                .sum::<usize>()
        });
        report("compress, fresh", t, Some(n));
        let (t, n) = best(ROUNDS, || {
            let mut c = Compress::new(level, false);
            pieces
                .iter()
                .map(|p| {
                    c.reset();
                    compress(&mut c, p)
                })
                .sum::<usize>()
        });
        report("compress, reset", t, Some(n));

        let mut out = vec![0; size];
        let mut decompress = |d: &mut Decompress, z: &[u8]| {
            let st = d.decompress(z, &mut out, FlushDecompress::Finish);
            assert_eq!(st.unwrap(), Status::StreamEnd);
            assert_eq!(d.total_out() as usize, size);
        };
        let (t, ()) = best(ROUNDS, || {
            for z in &deflated {
                decompress(&mut black_box(Decompress::new(false)), z);
            }
        });
        report("decompress, fresh", t, None);
        let (t, ()) = best(ROUNDS, || {
            let mut d = Decompress::new(false);
            for z in &deflated {
                d.reset(false);
                decompress(&mut d, z);
            }
        });
        report("decompress, reset", t, None);
    }
}
