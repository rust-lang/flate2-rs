#![allow(unexpected_cfgs)]

use flate2::write::ZlibEncoder;
use flate2::{
    read::ZlibDecoder, Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status,
};
use std::env;
use std::hint::black_box;
use std::io::{Read, Write};
use std::time::{Duration, Instant};
#[cfg(flate2_has_uninit_api)]
use std::mem::MaybeUninit;

const DRIVER_LABEL_ENV: &str = "FLATE2_BACKEND_BENCH_LABEL";
const CHUNK_IN: usize = 2 * 1024;
const CHUNK_OUT: usize = 2 * 1024 * 1024;
const PLAIN_LEN: usize = 16 * 1024 * 1024;
const BENCH_TARGET_SAMPLE_TIME: Duration = Duration::from_millis(200);
const BENCH_SAMPLES: usize = 5;
const BENCH_MAX_ITERS_PER_SAMPLE: usize = 12;

struct BenchmarkData {
    plain: Vec<u8>,
    zlib: Vec<u8>,
}

struct DriverResult {
    iterations_per_sample: usize,
    samples: usize,
    ns_per_byte: f64,
    measurement_uncertainty: f64,
}

fn benchmark_data() -> BenchmarkData {
    let line =
        b"The quick brown fox jumps over the lazy dog. 0123456789 abcdefghijklmnopqrstuvwxyz\n";
    let mut plain = Vec::with_capacity(PLAIN_LEN);
    while plain.len() < PLAIN_LEN {
        plain.extend_from_slice(line);
    }
    plain.truncate(PLAIN_LEN);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&plain).unwrap();
    let zlib = encoder.finish().unwrap();

    BenchmarkData { plain, zlib }
}

fn run_decompress_chunked_large_output_buf(data: &BenchmarkData) {
    let mut decoder = Decompress::new(true);
    let mut chunk = vec![0u8; CHUNK_OUT].into_boxed_slice();
    let mut result = Vec::with_capacity(data.plain.len());
    loop {
        let prior_out = decoder.total_out();
        let in_start = decoder.total_in() as usize;
        let in_end = (in_start + CHUNK_IN).min(data.zlib.len());
        let status = decoder
            .decompress(
                &data.zlib[in_start..in_end],
                &mut chunk,
                FlushDecompress::None,
            )
            .unwrap();
        let bytes_written = (decoder.total_out() - prior_out) as usize;
        result.extend_from_slice(&chunk[..bytes_written]);
        if status == Status::StreamEnd {
            break;
        }
    }
    assert_eq!(result, data.plain);
}

#[cfg_attr(not(flate2_compare_uninit_cases), allow(dead_code))]
#[cfg(flate2_has_uninit_api)]
fn initialized_prefix(output: &[MaybeUninit<u8>], bytes_written: usize) -> &[u8] {
    unsafe { std::slice::from_raw_parts(output.as_ptr() as *const u8, bytes_written) }
}

#[cfg_attr(not(flate2_compare_uninit_cases), allow(dead_code))]
#[cfg(flate2_has_uninit_api)]
fn run_decompress_uninit_chunked_large_output_buf(data: &BenchmarkData) {
    let mut decoder = Decompress::new(true);
    let mut chunk = vec![MaybeUninit::<u8>::uninit(); CHUNK_OUT].into_boxed_slice();
    let mut result = Vec::with_capacity(data.plain.len());
    loop {
        let prior_out = decoder.total_out();
        let in_start = decoder.total_in() as usize;
        let in_end = (in_start + CHUNK_IN).min(data.zlib.len());
        let status = decoder
            .decompress_uninit(
                &data.zlib[in_start..in_end],
                &mut chunk,
                FlushDecompress::None,
            )
            .unwrap();
        let bytes_written = (decoder.total_out() - prior_out) as usize;
        result.extend_from_slice(initialized_prefix(&chunk, bytes_written));
        if status == Status::StreamEnd {
            break;
        }
    }
    assert_eq!(result, data.plain);
}

#[cfg_attr(not(flate2_compare_uninit_cases), allow(dead_code))]
#[cfg(not(flate2_has_uninit_api))]
fn run_decompress_uninit_chunked_large_output_buf(data: &BenchmarkData) {
    run_decompress_chunked_large_output_buf(data);
}

fn run_compress_chunked_large_output_buf(data: &BenchmarkData) {
    let mut encoder = Compress::new(Compression::fast(), true);
    let mut chunk = vec![0u8; CHUNK_OUT].into_boxed_slice();
    let mut result = Vec::with_capacity(data.zlib.len() * 2);
    loop {
        let prior_out = encoder.total_out();
        let in_start = encoder.total_in() as usize;
        let in_end = (in_start + CHUNK_IN).min(data.plain.len());
        let flush = if in_end == data.plain.len() {
            FlushCompress::Finish
        } else {
            FlushCompress::None
        };
        let status = encoder
            .compress(&data.plain[in_start..in_end], &mut chunk, flush)
            .unwrap();
        let bytes_written = (encoder.total_out() - prior_out) as usize;
        result.extend_from_slice(&chunk[..bytes_written]);
        if status == Status::StreamEnd {
            break;
        }
    }

    let mut decoder = ZlibDecoder::new(result.as_slice());
    let mut decoded = Vec::with_capacity(data.plain.len());
    decoder.read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, data.plain);
}

#[cfg_attr(not(flate2_compare_uninit_cases), allow(dead_code))]
#[cfg(flate2_has_uninit_api)]
fn run_compress_uninit_chunked_large_output_buf(data: &BenchmarkData) {
    let mut encoder = Compress::new(Compression::fast(), true);
    let mut chunk = vec![MaybeUninit::<u8>::uninit(); CHUNK_OUT].into_boxed_slice();
    let mut result = Vec::with_capacity(data.zlib.len() * 2);
    loop {
        let prior_out = encoder.total_out();
        let in_start = encoder.total_in() as usize;
        let in_end = (in_start + CHUNK_IN).min(data.plain.len());
        let flush = if in_end == data.plain.len() {
            FlushCompress::Finish
        } else {
            FlushCompress::None
        };
        let status = encoder
            .compress_uninit(&data.plain[in_start..in_end], &mut chunk, flush)
            .unwrap();
        let bytes_written = (encoder.total_out() - prior_out) as usize;
        result.extend_from_slice(initialized_prefix(&chunk, bytes_written));
        if status == Status::StreamEnd {
            break;
        }
    }

    let mut decoder = ZlibDecoder::new(result.as_slice());
    let mut decoded = Vec::with_capacity(data.plain.len());
    decoder.read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, data.plain);
}

#[cfg_attr(not(flate2_compare_uninit_cases), allow(dead_code))]
#[cfg(not(flate2_has_uninit_api))]
fn run_compress_uninit_chunked_large_output_buf(data: &BenchmarkData) {
    run_compress_chunked_large_output_buf(data);
}

fn benchmark_case(data: &BenchmarkData, run: fn(&BenchmarkData)) -> DriverResult {
    let warmup_started = Instant::now();
    run(data);
    let warmup_elapsed = warmup_started.elapsed();
    let warmup_nanos = warmup_elapsed.as_nanos();
    let target_nanos = BENCH_TARGET_SAMPLE_TIME.as_nanos();
    let iterations_per_sample = if warmup_nanos == 0 {
        BENCH_MAX_ITERS_PER_SAMPLE
    } else {
        (target_nanos / warmup_nanos) as usize
    }
    .clamp(1, BENCH_MAX_ITERS_PER_SAMPLE);

    let mut samples = Vec::with_capacity(BENCH_SAMPLES);
    for _ in 0..BENCH_SAMPLES {
        let started = Instant::now();
        for _ in 0..iterations_per_sample {
            run(black_box(data));
        }
        samples.push(started.elapsed());
    }
    samples.sort_unstable();

    let iterations = iterations_per_sample as f64;
    let bytes = data.plain.len() as f64;
    let median = samples[samples.len() / 2];
    let ns_per_byte = duration_ns_per_byte(median, iterations, bytes);
    let measurement_uncertainty =
        relative_measurement_uncertainty(ns_per_byte, &samples, iterations, bytes);

    DriverResult {
        iterations_per_sample,
        samples: BENCH_SAMPLES,
        ns_per_byte,
        measurement_uncertainty,
    }
}

fn duration_ns_per_byte(duration: Duration, iterations: f64, bytes: f64) -> f64 {
    duration.as_nanos() as f64 / (iterations * bytes)
}

fn relative_measurement_uncertainty(
    median_ns_per_byte: f64,
    samples: &[Duration],
    iterations: f64,
    bytes: f64,
) -> f64 {
    samples
        .iter()
        .map(|sample| {
            let sample_ns_per_byte = duration_ns_per_byte(*sample, iterations, bytes);
            ((sample_ns_per_byte - median_ns_per_byte) / median_ns_per_byte).abs()
        })
        .fold(0.0_f64, f64::max)
}

#[cfg(feature = "zlib-ng")]
fn backend_name() -> &'static str {
    "zlib-ng"
}

#[cfg(all(not(feature = "zlib-ng"), feature = "zlib-ng-compat"))]
fn backend_name() -> &'static str {
    "zlib-ng-compat"
}

#[cfg(all(
    not(feature = "zlib-ng"),
    not(feature = "zlib-ng-compat"),
    feature = "zlib-rs"
))]
fn backend_name() -> &'static str {
    "zlib-rs"
}

#[cfg(all(
    not(feature = "zlib-ng"),
    not(feature = "zlib-ng-compat"),
    not(feature = "zlib-rs"),
    feature = "zlib"
))]
fn backend_name() -> &'static str {
    "zlib"
}

#[cfg(all(
    not(feature = "zlib-ng"),
    not(feature = "zlib-ng-compat"),
    not(feature = "zlib-rs"),
    not(feature = "zlib")
))]
fn backend_name() -> &'static str {
    "rust_backend"
}

fn driver_label() -> String {
    env::var(DRIVER_LABEL_ENV).unwrap_or_else(|_| "benchmark target".to_owned())
}

fn print_record(backend: &str, case: &str, result: &DriverResult) {
    println!(
        "{backend},{case},{},{},{:.9},{:.6}",
        result.iterations_per_sample,
        result.samples,
        result.ns_per_byte,
        result.measurement_uncertainty,
    );
}

fn main() {
    let backend = backend_name();
    let label = driver_label();
    eprintln!("[backend-driver] measuring {label} for {backend}");
    let data = benchmark_data();
    let compress = benchmark_case(&data, run_compress_chunked_large_output_buf);
    let decompress = benchmark_case(&data, run_decompress_chunked_large_output_buf);
    #[cfg(flate2_compare_uninit_cases)]
    let compress_uninit = benchmark_case(&data, run_compress_uninit_chunked_large_output_buf);
    #[cfg(flate2_compare_uninit_cases)]
    let decompress_uninit = benchmark_case(&data, run_decompress_uninit_chunked_large_output_buf);

    eprintln!("[backend-driver] emitting benchmark CSV for {label} and {backend}");
    println!("backend,case,iterations_per_sample,samples,ns_per_byte,measurement_uncertainty");
    print_record(backend, "compress_chunked_large_output_buf", &compress);
    print_record(backend, "decompress_chunked_large_output_buf", &decompress);
    #[cfg(flate2_compare_uninit_cases)]
    print_record(
        backend,
        "compress_uninit_chunked_large_output_buf",
        &compress_uninit,
    );
    #[cfg(flate2_compare_uninit_cases)]
    print_record(
        backend,
        "decompress_uninit_chunked_large_output_buf",
        &decompress_uninit,
    );
}
