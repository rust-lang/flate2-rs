//! Implementation for `fdeflate` rust backend.

use std::fmt;

use super::*;
use crate::mem;

// fdeflate doesn't provide error messages
#[derive(Clone, Default)]
pub struct ErrorMessage;

impl ErrorMessage {
    pub fn get(&self) -> Option<&str> {
        None
    }
}

// Constants matching the zlib flush values used by FlushCompress/FlushDecompress.
// These are defined as isize to match the interface expected by mem.rs for the
// FlushCompress/FlushDecompress enum discriminants.
pub const MZ_NO_FLUSH: isize = 0;
pub const MZ_PARTIAL_FLUSH: isize = 1;
pub const MZ_SYNC_FLUSH: isize = 2;
pub const MZ_FULL_FLUSH: isize = 3;
pub const MZ_FINISH: isize = 4;
pub const MZ_DEFAULT_WINDOW_BITS: isize = 15;

/// The DEFLATE window size (32 KB). fdeflate's Decompressor uses the output buffer
/// as a lookback window for back-references, so we must maintain a persistent buffer
/// that contains at least this much history.
const WINDOW_SIZE: usize = 32768;

pub struct Inflate {
    inner: ::fdeflate::Decompressor,
    /// Persistent output buffer used as the lookback window. fdeflate reads
    /// back-references from this buffer, so we must keep prior output available.
    window_buf: Vec<u8>,
    /// Current write position in `window_buf`.
    window_pos: usize,
    total_in: u64,
    total_out: u64,
    is_zlib: bool,
}

impl fmt::Debug for Inflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "fdeflate inflate internal state. total_in: {}, total_out: {}",
            self.total_in, self.total_out,
        )
    }
}

impl Inflate {
    /// Ensure there is room to write `needed` bytes into the window buffer.
    /// If the buffer is getting too large, shift it to keep only the last WINDOW_SIZE bytes.
    fn ensure_window_capacity(&mut self, needed: usize) {
        if self.window_pos + needed > self.window_buf.len() {
            // Grow the buffer to accommodate the new data
            self.window_buf.resize(self.window_pos + needed, 0);
        }

        // Compact when the buffer gets much larger than the window size
        if self.window_pos > WINDOW_SIZE * 4 {
            let keep_from = self.window_pos.saturating_sub(WINDOW_SIZE);
            self.window_buf.copy_within(keep_from..self.window_pos, 0);
            self.window_pos -= keep_from;
            self.window_buf.truncate(self.window_pos + needed);
        }
    }
}

impl InflateBackend for Inflate {
    fn make(zlib_header: bool, _window_bits: u8) -> Self {
        Inflate {
            inner: if zlib_header {
                ::fdeflate::Decompressor::new()
            } else {
                ::fdeflate::Decompressor::new_raw()
            },
            window_buf: Vec::new(),
            window_pos: 0,
            total_in: 0,
            total_out: 0,
            is_zlib: zlib_header,
        }
    }

    fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        _flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        if self.inner.is_done() {
            return Ok(Status::StreamEnd);
        }

        // Make sure we have space in the window buffer for the caller's output size
        self.ensure_window_capacity(output.len());

        let (consumed, produced) = match self
            .inner
            .read(input, &mut self.window_buf, self.window_pos)
        {
            Ok(result) => result,
            Err(_) => return mem::decompress_failed(ErrorMessage),
        };

        // Copy newly produced bytes to the caller's output buffer
        output[..produced]
            .copy_from_slice(&self.window_buf[self.window_pos..self.window_pos + produced]);
        self.window_pos += produced;

        // When the decompressor finishes, its bit buffer may contain bytes
        // that were read from input but not actually consumed. Subtract those
        // so that total_in accurately reflects the compressed stream boundary.
        let over_read = if self.inner.is_done() {
            self.inner.unconsumed_bytes()
        } else {
            0
        };

        self.total_in += (consumed - over_read) as u64;
        self.total_out += produced as u64;

        if self.inner.is_done() {
            return Ok(Status::StreamEnd);
        }

        if consumed == 0 && produced == 0 {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }

    fn reset(&mut self, zlib_header: bool) {
        self.is_zlib = zlib_header;
        self.inner = if zlib_header {
            ::fdeflate::Decompressor::new()
        } else {
            ::fdeflate::Decompressor::new_raw()
        };
        self.window_buf.clear();
        self.window_pos = 0;
        self.total_in = 0;
        self.total_out = 0;
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

/// Wraps fdeflate's Compressor to implement the streaming DeflateBackend interface.
///
/// fdeflate's Compressor takes a `Write` target and has `write_data`/`finish` methods.
/// We use a `Vec<u8>` as the internal output buffer. Since fdeflate writes compressed
/// data to the writer during `write_data` and `finish` calls, we buffer that output
/// and drain it into the caller's output buffer on each `compress` call.
pub struct Deflate {
    /// Buffered compressed output that hasn't been returned to the caller yet.
    output_buf: Vec<u8>,
    /// The compressor state. Set to `None` after `finish()` is called.
    inner: Option<::fdeflate::Compressor<Vec<u8>>>,
    total_in: u64,
    total_out: u64,
    level: u8,
    zlib_header: bool,
}

impl fmt::Debug for Deflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "fdeflate deflate internal state. total_in: {}, total_out: {}",
            self.total_in, self.total_out,
        )
    }
}

impl DeflateBackend for Deflate {
    fn make(level: Compression, zlib_header: bool, _window_bits: u8) -> Self {
        let level_u8 = level.level().min(9) as u8;
        let compressor =
            ::fdeflate::Compressor::new(Vec::new(), level_u8, zlib_header).unwrap();

        Deflate {
            output_buf: Vec::new(),
            inner: Some(compressor),
            total_in: 0,
            total_out: 0,
            level: level_u8,
            zlib_header,
        }
    }

    fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        // If we have buffered output from a previous call, drain that first
        // before processing new input.
        if !self.output_buf.is_empty() {
            let copy_len = self.output_buf.len().min(output.len());
            output[..copy_len].copy_from_slice(&self.output_buf[..copy_len]);
            self.output_buf.drain(..copy_len);
            self.total_out += copy_len as u64;

            if !self.output_buf.is_empty() {
                // Still more buffered output to drain, don't consume any input
                return Ok(Status::Ok);
            }

            // If we already finished and the buffer is now drained, we're done.
            if self.inner.is_none() {
                return Ok(Status::StreamEnd);
            }

            // Buffer drained but no new input and not finishing - signal caller
            if input.is_empty() && flush == FlushCompress::None {
                return Ok(Status::Ok);
            }
        }

        // If the compressor has already been finished and there's nothing
        // left to drain, signal completion.
        if self.inner.is_none() {
            return Ok(Status::StreamEnd);
        }

        // Feed input to the compressor
        if !input.is_empty() {
            let compressor = self.inner.as_mut().unwrap();
            if compressor.write_data(input).is_err() {
                return mem::compress_failed(ErrorMessage);
            }
            self.total_in += input.len() as u64;
        }

        if flush == FlushCompress::Finish {
            // Finalize the compressor and collect all remaining output
            let compressor = self.inner.take().unwrap();
            let compressed = match compressor.finish() {
                Ok(c) => c,
                Err(_) => return mem::compress_failed(ErrorMessage),
            };

            let copy_len = compressed.len().min(output.len());
            output[..copy_len].copy_from_slice(&compressed[..copy_len]);
            self.total_out += copy_len as u64;

            if copy_len < compressed.len() {
                self.output_buf.extend_from_slice(&compressed[copy_len..]);
                return Ok(Status::Ok);
            }

            return Ok(Status::StreamEnd);
        }

        // For non-Finish calls: fdeflate buffers compressed data internally in
        // the Vec<u8> writer. We can't extract partial output without finishing,
        // so we just report that input was consumed. The compressed output will
        // become available when finish() is eventually called.
        if input.is_empty() {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }

    fn reset(&mut self) {
        self.total_in = 0;
        self.total_out = 0;
        self.output_buf.clear();
        self.inner = Some(
            ::fdeflate::Compressor::new(Vec::new(), self.level, self.zlib_header).unwrap(),
        );
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
