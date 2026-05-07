//! Implementation for `zlib_rs` rust backend.
//!
//! Every backend must provide two types:
//!
//! - `Deflate` for compression, implements the `Backend` and `DeflateBackend` trait
//! - `Inflate` for decompression, implements the `Backend` and `InflateBackend` trait
//!
//! Additionally the backend provides a number of constants, and a `ErrorMessage` type.
//!
//! ## Allocation
//!
//! The (de)compression state is not boxed. The C implementations require that the z_stream is
//! pinned in memory (has a fixed address), because their z_stream is self-referential. The most
//! convenient way in rust to guarantee a stable address is to `Box` the data, but it does add an
//! additional allocation.
//!
//! With zlib_rs the state is not self-referential and hence no boxing is needed. The `new` methods
//! internally do allocate space for the (de)compression state.

use std::{ffi::CStr, fmt, mem::MaybeUninit};

use ::zlib_rs::{
    c_api::z_stream,
    deflate::{self, DeflateStream},
    inflate::{self, InflateStream},
    DeflateConfig, DeflateFlush, InflateConfig, InflateFlush, ReturnCode,
};

pub const MZ_NO_FLUSH: isize = DeflateFlush::NoFlush as isize;
pub const MZ_PARTIAL_FLUSH: isize = DeflateFlush::PartialFlush as isize;
pub const MZ_SYNC_FLUSH: isize = DeflateFlush::SyncFlush as isize;
pub const MZ_FULL_FLUSH: isize = DeflateFlush::FullFlush as isize;
pub const MZ_FINISH: isize = DeflateFlush::Finish as isize;

pub const MZ_DEFAULT_WINDOW_BITS: core::ffi::c_int = 15;

use super::*;
use crate::mem::{compress_failed, decompress_failed};

#[derive(Clone, Default)]
pub struct ErrorMessage(Option<&'static str>);

impl ErrorMessage {
    pub fn get(&self) -> Option<&str> {
        self.0
    }
}

pub struct Inflate {
    pub(crate) inner: z_stream,
    // NOTE: these counts do not count the dictionary.
    total_in: u64,
    total_out: u64,
}

unsafe impl Send for Inflate {}
unsafe impl Sync for Inflate {}

impl fmt::Debug for Inflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "zlib_rs inflate internal state. total_in: {}, total_out: {}",
            self.total_in(),
            self.total_out(),
        )
    }
}

impl InflateBackend for Inflate {
    fn make(zlib_header: bool, window_bits: u8) -> Self {
        let config = InflateConfig {
            window_bits: if zlib_header {
                i32::from(window_bits)
            } else {
                -i32::from(window_bits)
            },
        };

        Inflate {
            inner: stream_with_inflate_config(config),
            total_in: 0,
            total_out: 0,
        }
    }

    fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        self.decompress_impl(input, output.as_mut_ptr(), output.len(), flush)
    }

    fn decompress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        self.decompress_impl(input, output.as_mut_ptr().cast::<u8>(), output.len(), flush)
    }

    fn reset(&mut self, zlib_header: bool) {
        self.total_in = 0;
        self.total_out = 0;

        let mut config = InflateConfig::default();
        if !zlib_header {
            config.window_bits = -config.window_bits;
        }

        let _ = inflate::reset_with_config(self.stream_mut(), config);
    }
}

impl Backend for Inflate {
    #[inline]
    fn total_in(&self) -> u64 {
        self.total_in
    }

    #[inline]
    fn total_out(&self) -> u64 {
        self.total_out
    }
}

impl Inflate {
    fn decompress_impl(
        &mut self,
        input: &[u8],
        output_ptr: *mut u8,
        output_len: usize,
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        let flush = match flush {
            FlushDecompress::None => InflateFlush::NoFlush,
            FlushDecompress::Sync => InflateFlush::SyncFlush,
            FlushDecompress::Finish => InflateFlush::Finish,
        };

        let total_in_start = self.inner.total_in;
        let total_out_start = self.inner.total_out;

        self.inner.avail_in = Ord::min(input.len(), u32::MAX as usize) as u32;
        self.inner.avail_out = Ord::min(output_len, u32::MAX as usize) as u32;
        self.inner.next_in = input.as_ptr();
        self.inner.next_out = output_ptr;

        let result = unsafe { inflate::inflate(self.stream_mut(), flush) };

        self.accumulate_totals(total_in_start, total_out_start);

        match result {
            ReturnCode::Ok => Ok(Status::Ok),
            ReturnCode::StreamEnd => Ok(Status::StreamEnd),
            ReturnCode::BufError => Ok(Status::BufError),
            ReturnCode::NeedDict => crate::mem::decompress_need_dict(self.inner.adler as u32),
            ReturnCode::ErrNo | ReturnCode::VersionError => unreachable!(),
            ReturnCode::StreamError | ReturnCode::DataError | ReturnCode::MemError => {
                self.decompress_error()
            }
        }
    }

    fn accumulate_totals(&mut self, total_in_start: u64, total_out_start: u64) {
        self.total_in += self.inner.total_in - total_in_start;
        self.total_out += self.inner.total_out - total_out_start;
    }

    fn stream_mut(&mut self) -> &mut InflateStream<'static> {
        unsafe { InflateStream::from_stream_mut(&mut self.inner) }
            .expect("zlib-rs inflate stream is initialized")
    }

    fn decompress_error<T>(&self) -> Result<T, DecompressError> {
        decompress_failed(ErrorMessage(error_message(self.inner.msg)))
    }

    pub fn set_dictionary(&mut self, dictionary: &[u8]) -> Result<u32, DecompressError> {
        match inflate::set_dictionary(self.stream_mut(), dictionary) {
            ReturnCode::Ok => Ok(self.inner.adler as u32),
            ReturnCode::StreamError | ReturnCode::DataError => self.decompress_error(),
            _other => unreachable!("set_dictionary does not return {:?}", _other),
        }
    }
}

impl Drop for Inflate {
    fn drop(&mut self) {
        if let Some(stream) = unsafe { InflateStream::from_stream_mut(&mut self.inner) } {
            let _ = inflate::end(stream);
        }
    }
}

pub struct Deflate {
    pub(crate) inner: z_stream,
    // NOTE: these counts do not count the dictionary.
    total_in: u64,
    total_out: u64,
}

unsafe impl Send for Deflate {}
unsafe impl Sync for Deflate {}

impl fmt::Debug for Deflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "zlib_rs deflate internal state. total_in: {}, total_out: {}",
            self.total_in(),
            self.total_out(),
        )
    }
}

impl DeflateBackend for Deflate {
    fn make(level: Compression, zlib_header: bool, window_bits: u8) -> Self {
        debug_assert!(level.level() <= 9);

        let config = DeflateConfig {
            window_bits: if zlib_header {
                i32::from(window_bits)
            } else {
                -i32::from(window_bits)
            },
            level: level.level() as i32,
            ..DeflateConfig::default()
        };

        Deflate {
            inner: stream_with_deflate_config(config),
            total_in: 0,
            total_out: 0,
        }
    }

    fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        self.compress_impl(input, output.as_mut_ptr(), output.len(), flush)
    }

    fn compress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        self.compress_impl(input, output.as_mut_ptr().cast::<u8>(), output.len(), flush)
    }

    fn reset(&mut self) {
        self.total_in = 0;
        self.total_out = 0;
        let _ = deflate::reset(self.stream_mut());
    }
}

impl Backend for Deflate {
    #[inline]
    fn total_in(&self) -> u64 {
        self.total_in
    }

    #[inline]
    fn total_out(&self) -> u64 {
        self.total_out
    }
}

impl Deflate {
    fn compress_impl(
        &mut self,
        input: &[u8],
        output_ptr: *mut u8,
        output_len: usize,
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        let flush = match flush {
            FlushCompress::None => DeflateFlush::NoFlush,
            FlushCompress::Partial => DeflateFlush::PartialFlush,
            FlushCompress::Sync => DeflateFlush::SyncFlush,
            FlushCompress::Full => DeflateFlush::FullFlush,
            FlushCompress::Finish => DeflateFlush::Finish,
        };

        let total_in_start = self.inner.total_in;
        let total_out_start = self.inner.total_out;

        self.inner.avail_in = Ord::min(input.len(), u32::MAX as usize) as u32;
        self.inner.avail_out = Ord::min(output_len, u32::MAX as usize) as u32;
        self.inner.next_in = input.as_ptr();
        self.inner.next_out = output_ptr;

        let result = deflate::deflate(self.stream_mut(), flush);

        self.accumulate_totals(total_in_start, total_out_start);

        match result {
            ReturnCode::Ok => Ok(Status::Ok),
            ReturnCode::StreamEnd => Ok(Status::StreamEnd),
            ReturnCode::BufError => Ok(Status::BufError),
            ReturnCode::NeedDict | ReturnCode::ErrNo | ReturnCode::VersionError => unreachable!(),
            ReturnCode::StreamError | ReturnCode::DataError | ReturnCode::MemError => {
                self.compress_error()
            }
        }
    }

    fn accumulate_totals(&mut self, total_in_start: u64, total_out_start: u64) {
        self.total_in += self.inner.total_in - total_in_start;
        self.total_out += self.inner.total_out - total_out_start;
    }

    fn stream_mut(&mut self) -> &mut DeflateStream<'static> {
        unsafe { DeflateStream::from_stream_mut(&mut self.inner) }
            .expect("zlib-rs deflate stream is initialized")
    }

    fn compress_error<T>(&self) -> Result<T, CompressError> {
        compress_failed(ErrorMessage(error_message(self.inner.msg)))
    }

    pub fn set_dictionary(&mut self, dictionary: &[u8]) -> Result<u32, CompressError> {
        match deflate::set_dictionary(self.stream_mut(), dictionary) {
            ReturnCode::Ok => Ok(self.inner.adler as u32),
            ReturnCode::StreamError => self.compress_error(),
            _other => unreachable!("set_dictionary does not return {:?}", _other),
        }
    }

    pub fn set_level(&mut self, level: Compression) -> Result<(), CompressError> {
        match deflate::params(self.stream_mut(), level.level() as i32, Default::default()) {
            ReturnCode::Ok => Ok(()),
            ReturnCode::BufError => compress_failed(ErrorMessage(Some("insufficient space"))),
            ReturnCode::StreamError => self.compress_error(),
            _other => unreachable!("set_level does not return {:?}", _other),
        }
    }
}

impl Drop for Deflate {
    fn drop(&mut self) {
        if let Some(stream) = unsafe { DeflateStream::from_stream_mut(&mut self.inner) } {
            let _ = deflate::end(stream);
        }
    }
}

fn error_message(msg: *const core::ffi::c_char) -> Option<&'static str> {
    if msg.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(msg).to_str() }.ok()
    }
}

fn stream_with_inflate_config(config: InflateConfig) -> z_stream {
    let mut stream = z_stream::default();
    let result = inflate::init(&mut stream, config);
    assert_eq!(result, ReturnCode::Ok);
    stream
}

fn stream_with_deflate_config(config: DeflateConfig) -> z_stream {
    let mut stream = z_stream::default();
    let result = deflate::init(&mut stream, config);
    assert_eq!(result, ReturnCode::Ok);
    stream
}
