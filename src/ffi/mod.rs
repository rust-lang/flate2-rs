//! This module contains backend-specific code.

use crate::mem::{CompressError, DecompressError, FlushCompress, FlushDecompress, Status};
use crate::Compression;
use std::mem::MaybeUninit;

fn initialize_buffer(output: &mut [MaybeUninit<u8>]) -> &mut [u8] {
    // SAFETY: Here we zero-initialize the output and cast it to [u8]
    unsafe {
        output.as_mut_ptr().write_bytes(0, output.len());
        &mut *(output as *mut [MaybeUninit<u8>] as *mut [u8])
    }
}

/// Traits specifying the interface of the backends.
///
/// Sync + Send are added as a condition to ensure they are available
/// for the frontend.
pub trait Backend: Sync + Send {
    fn total_in(&self) -> u64;
    fn total_out(&self) -> u64;
}

pub trait InflateBackend: Backend {
    fn make(zlib_header: bool, window_bits: u8) -> Self;
    fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError>;
    fn decompress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        self.decompress(input, initialize_buffer(output), flush)
    }
    fn reset(&mut self, zlib_header: bool);
}

pub trait DeflateBackend: Backend {
    fn make(level: Compression, zlib_header: bool, window_bits: u8) -> Self;
    fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushCompress,
    ) -> Result<Status, CompressError>;
    fn compress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        self.compress(input, initialize_buffer(output), flush)
    }
    fn reset(&mut self);
}

// Select the actual implementation that is used.
//
// This choice is straightforward if only one backend is enabled, but the ordering of the branches
// matters when multiple backends are selected (e.g. through feature unification). The top branch
// has the highest priority, the bottom branch the lowest.
crate::cfg_select!(
    feature = "any_c_zlib" => {
        // Use a C backend when explicitly selected.
        mod c;
        pub use self::c::*;
    }
    feature = "miniz_oxide" => {
        // Only bring in `miniz_oxide` if there is no C-based backend.
        mod miniz_oxide;
        pub use self::miniz_oxide::*;
    }
    feature = "zlib-rs" => {
        // Only use `zlib_rs` when no other backend is explicitly selected.
        mod zlib_rs;
        pub use self::zlib_rs::*;
    }
    _ => {
        // If no backend is enabled, fail fast with a clear error message.
        compile_error!("No compression backend selected; enable one of `zlib`, `zlib-ng`, `zlib-rs`, `miniz_oxide`, or the default `rust_backend` feature.");
    }
);

impl std::fmt::Debug for ErrorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.get().fmt(f)
    }
}
