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
/// fdeflate's Compressor writes compressed data to its inner `Vec<u8>` writer
/// incrementally during `write_data()` calls. After each call, we drain the
/// writer's buffer directly into the caller's output slice, avoiding any
/// secondary buffering.
///
/// After `finish()` is called, any remaining compressed data that didn't fit
/// in the caller's output is held in `finished_buf` until drained.
pub struct Deflate {
    /// The compressor state. Set to `None` after `finish()` is called.
    inner: Option<::fdeflate::Compressor<Vec<u8>>>,
    /// Holds leftover compressed data after `finish()` that didn't fit in the
    /// caller's output. Only populated once the compressor is consumed.
    finished_buf: Vec<u8>,
    /// Read position within `finished_buf`.
    finished_pos: usize,
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

/// Drain as many bytes as possible from `src[*pos..]` into `output`,
/// returning the number of bytes copied.
fn drain_to_output(src: &[u8], pos: &mut usize, output: &mut [u8]) -> usize {
    let available = src.len() - *pos;
    let copy_len = available.min(output.len());
    output[..copy_len].copy_from_slice(&src[*pos..*pos + copy_len]);
    *pos += copy_len;
    copy_len
}

impl DeflateBackend for Deflate {
    fn make(level: Compression, zlib_header: bool, _window_bits: u8) -> Self {
        let level_u8 = level.level().min(9) as u8;
        let compressor =
            ::fdeflate::Compressor::new(Vec::new(), level_u8, zlib_header).unwrap();

        Deflate {
            inner: Some(compressor),
            finished_buf: Vec::new(),
            finished_pos: 0,
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
        // After finish(), drain any remaining compressed data.
        if self.inner.is_none() {
            if self.finished_pos < self.finished_buf.len() {
                let n = drain_to_output(&self.finished_buf, &mut self.finished_pos, output);
                self.total_out += n as u64;
                if self.finished_pos < self.finished_buf.len() {
                    return Ok(Status::Ok);
                }
            }
            return Ok(Status::StreamEnd);
        }

        let compressor = self.inner.as_mut().unwrap();

        // Drain any compressed data already sitting in the writer from a
        // previous write_data() call before feeding new input.
        let mut out_pos = 0;
        {
            let writer = compressor.get_writer_mut();
            let n = drain_to_output(writer, &mut 0, &mut output[out_pos..]);
            // Remove the drained bytes from the front of the writer Vec.
            if n > 0 {
                writer.drain(..n);
            }
            self.total_out += n as u64;
            out_pos += n;
        }

        // Feed input to the compressor.
        if !input.is_empty() {
            if compressor.write_data(input).is_err() {
                return mem::compress_failed(ErrorMessage);
            }
            self.total_in += input.len() as u64;

            // Drain newly produced compressed data into the remaining output space.
            let writer = compressor.get_writer_mut();
            let n = drain_to_output(writer, &mut 0, &mut output[out_pos..]);
            if n > 0 {
                writer.drain(..n);
            }
            self.total_out += n as u64;
            out_pos += n;
        }

        if flush == FlushCompress::Finish {
            // Finalize the compressor and collect all remaining output.
            let compressor = self.inner.take().unwrap();
            let compressed = match compressor.finish() {
                Ok(c) => c,
                Err(_) => return mem::compress_failed(ErrorMessage),
            };

            let n = drain_to_output(&compressed, &mut 0, &mut output[out_pos..]);
            self.total_out += n as u64;

            if n < compressed.len() {
                // Couldn't fit everything — stash the remainder.
                self.finished_buf = compressed;
                self.finished_pos = n;
                return Ok(Status::Ok);
            }

            return Ok(Status::StreamEnd);
        }

        if out_pos == 0 && input.is_empty() {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }

    fn reset(&mut self) {
        self.total_in = 0;
        self.total_out = 0;
        self.finished_buf.clear();
        self.finished_pos = 0;
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
