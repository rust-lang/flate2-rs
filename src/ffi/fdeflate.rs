//! Implementation for the `fdeflate` Rust backend.

use std::fmt;
use std::io;

use ::fdeflate::{DecompressionError as FdeflateError, FlushKind};

pub const MZ_NO_FLUSH: isize = 0;
pub const MZ_PARTIAL_FLUSH: isize = 1;
pub const MZ_SYNC_FLUSH: isize = 2;
pub const MZ_FULL_FLUSH: isize = 3;
pub const MZ_FINISH: isize = 4;

pub const MZ_DEFAULT_WINDOW_BITS: core::ffi::c_int = 15;

use super::*;
use crate::mem::{compress_failed, decompress_failed};

// fdeflate's decompressor needs one contiguous output slice containing recent history followed by
// writable space. Keeping 32 KiB of history plus a 64 KiB decode tail avoids copying the history on
// every call while still bounding how much decoded output we stage ahead of the caller.
const FDEFLATE_HISTORY_LEN: usize = 32 * 1024;
const FDEFLATE_DECODE_TAIL_LEN: usize = 64 * 1024;
const FDEFLATE_SCRATCH_LEN: usize = FDEFLATE_HISTORY_LEN + FDEFLATE_DECODE_TAIL_LEN;

#[derive(Clone, Default)]
pub struct ErrorMessage(Option<&'static str>);

impl ErrorMessage {
    pub fn get(&self) -> Option<&str> {
        self.0
    }
}

fn format_from_bool(zlib_header: bool) -> ::fdeflate::Format {
    if zlib_header {
        ::fdeflate::Format::Zlib
    } else {
        ::fdeflate::Format::Raw
    }
}

fn decompression_message(error: FdeflateError) -> &'static str {
    match error {
        FdeflateError::BadZlibHeader => "bad zlib header",
        FdeflateError::InsufficientInput => "insufficient input",
        FdeflateError::InvalidBlockType => "invalid block type",
        FdeflateError::InvalidUncompressedBlockLength => "invalid uncompressed block length",
        FdeflateError::InvalidHlit => "invalid literal/length code count",
        FdeflateError::InvalidHdist => "invalid distance code count",
        FdeflateError::InvalidCodeLengthRepeat => "invalid code length repeat",
        FdeflateError::BadCodeLengthHuffmanTree => "bad code length huffman tree",
        FdeflateError::BadLiteralLengthHuffmanTree => "bad literal/length huffman tree",
        FdeflateError::BadDistanceHuffmanTree => "bad distance huffman tree",
        FdeflateError::InvalidLiteralLengthCode => "invalid literal/length code",
        FdeflateError::InvalidDistanceCode => "invalid distance code",
        FdeflateError::InputStartsWithRun => "input starts with a run",
        FdeflateError::DistanceTooFarBack => "distance too far back",
        FdeflateError::WrongChecksum => "wrong checksum",
        FdeflateError::ExtraInput => "extra input",
    }
}

fn io_error_message(_: io::Error) -> ErrorMessage {
    ErrorMessage(Some("fdeflate compression failed"))
}

fn compression_result<T>(result: io::Result<T>) -> Result<T, CompressError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => compress_failed(io_error_message(error)),
    }
}

fn new_compressor(level: u8, zlib_header: bool) -> ::fdeflate::Compressor<Vec<u8>> {
    ::fdeflate::Compressor::new(Vec::new(), level, zlib_header)
        .expect("writing fdeflate output to a Vec cannot fail")
}

fn drain_vec(pending: &mut Vec<u8>, pending_pos: &mut usize, output: &mut [u8]) -> usize {
    let n = (pending.len() - *pending_pos).min(output.len());
    output[..n].copy_from_slice(&pending[*pending_pos..*pending_pos + n]);
    *pending_pos += n;
    if *pending_pos == pending.len() {
        pending.clear();
        *pending_pos = 0;
    }
    n
}

pub struct Deflate {
    inner: Option<::fdeflate::Compressor<Vec<u8>>>,
    pending: Vec<u8>,
    pending_pos: usize,
    inner_pending_pos: usize,
    level: u8,
    zlib_header: bool,
    total_in: u64,
    total_out: u64,
}

impl fmt::Debug for Deflate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "fdeflate deflate internal state. total_in: {}, total_out: {}",
            self.total_in, self.total_out,
        )
    }
}

impl Deflate {
    fn drain_pending(&mut self, output: &mut [u8]) -> usize {
        let mut output_index = drain_vec(&mut self.pending, &mut self.pending_pos, output);

        if output_index < output.len() {
            if let Some(ref mut inner) = self.inner {
                // fdeflate writes into the wrapped Vec; flate2 callers own the real destination, so
                // drain those pending bytes before accepting more input. Track a cursor instead of
                // draining the Vec so small caller buffers do not force repeated memmoves.
                output_index += drain_vec(
                    inner.get_mut(),
                    &mut self.inner_pending_pos,
                    &mut output[output_index..],
                );
            }
        }

        self.total_out += output_index as u64;
        output_index
    }

    fn is_finished(&self) -> bool {
        self.inner.is_none() && self.pending.is_empty()
    }
}

impl DeflateBackend for Deflate {
    fn make(level: Compression, zlib_header: bool, _window_bits: u8) -> Self {
        let level = level.level().min(9) as u8;
        Deflate {
            inner: Some(new_compressor(level, zlib_header)),
            pending: Vec::new(),
            pending_pos: 0,
            inner_pending_pos: 0,
            level,
            zlib_header,
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
        let before_in = self.total_in;
        let before_out = self.total_out;

        let output_index = self.drain_pending(output);
        if output_index == output.len() {
            return if self.is_finished() {
                Ok(Status::StreamEnd)
            } else if output.is_empty() {
                Ok(Status::BufError)
            } else {
                Ok(Status::Ok)
            };
        }

        if self.inner.is_none() {
            return Ok(Status::StreamEnd);
        }

        if let Some(ref mut inner) = self.inner {
            if !input.is_empty() {
                compression_result(inner.write_data(input))?;
                self.total_in += input.len() as u64;
            }

            match flush {
                FlushCompress::None => {}
                FlushCompress::Partial => compression_result(inner.flush(FlushKind::Partial))?,
                FlushCompress::Sync => compression_result(inner.flush(FlushKind::Sync))?,
                FlushCompress::Full => compression_result(inner.flush(FlushKind::Full))?,
                FlushCompress::Finish => {
                    let inner = self.inner.take().unwrap();
                    self.pending = compression_result(inner.finish())?;
                    self.pending_pos = 0;
                    self.inner_pending_pos = 0;
                }
            }
        }

        self.drain_pending(&mut output[output_index..]);

        if self.is_finished() {
            Ok(Status::StreamEnd)
        } else if before_in == self.total_in && before_out == self.total_out {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }

    fn reset(&mut self) {
        self.inner = Some(new_compressor(self.level, self.zlib_header));
        self.pending.clear();
        self.pending_pos = 0;
        self.inner_pending_pos = 0;
        self.total_in = 0;
        self.total_out = 0;
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

pub struct Inflate {
    inner: ::fdeflate::Decompressor,
    scratch: Vec<u8>,
    drain_pos: usize,
    decode_pos: usize,
    total_in: u64,
    total_out: u64,
    done: bool,
}

impl fmt::Debug for Inflate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "fdeflate inflate internal state. total_in: {}, total_out: {}",
            self.total_in, self.total_out,
        )
    }
}

impl Inflate {
    fn has_pending_output(&self) -> bool {
        self.drain_pos < self.decode_pos
    }

    fn drain_output(&mut self, output: &mut [u8]) -> usize {
        let n = (self.decode_pos - self.drain_pos).min(output.len());
        output[..n].copy_from_slice(&self.scratch[self.drain_pos..self.drain_pos + n]);
        self.drain_pos += n;
        self.total_out += n as u64;
        n
    }

    fn compact_if_needed(&mut self) {
        debug_assert!(!self.has_pending_output());

        if self.decode_pos + FDEFLATE_DECODE_TAIL_LEN <= self.scratch.len() {
            return;
        }

        // A VecDeque would avoid this occasional copy, but fdeflate needs a single contiguous
        // slice containing both lookback history and writable output. Once the staged output has
        // been drained, keep only the 32 KiB deflate history and reopen the 64 KiB decode tail.
        let keep = self.decode_pos.min(FDEFLATE_HISTORY_LEN);
        let start = self.decode_pos - keep;
        if start != 0 {
            self.scratch.copy_within(start..self.decode_pos, 0);
        }
        self.drain_pos = keep;
        self.decode_pos = keep;
    }
}

impl InflateBackend for Inflate {
    fn make(zlib_header: bool, _window_bits: u8) -> Self {
        let format = format_from_bool(zlib_header);
        Inflate {
            inner: ::fdeflate::Decompressor::new_with_format(format),
            scratch: vec![0; FDEFLATE_SCRATCH_LEN],
            drain_pos: 0,
            decode_pos: 0,
            total_in: 0,
            total_out: 0,
            done: false,
        }
    }

    fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        _flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        let before_in = self.total_in;
        let before_out = self.total_out;

        // Always drain staged output first. This keeps the scratch buffer from accumulating more
        // than the tunable 64 KiB decode tail when callers provide very small output buffers.
        let output_index = self.drain_output(output);
        if output_index == output.len() {
            return if self.done && !self.has_pending_output() {
                Ok(Status::StreamEnd)
            } else if output.is_empty() {
                Ok(Status::BufError)
            } else {
                Ok(Status::Ok)
            };
        }

        if self.done {
            return Ok(Status::StreamEnd);
        }

        self.compact_if_needed();

        let mut input_index = 0;
        loop {
            let target_end = (self.decode_pos + FDEFLATE_DECODE_TAIL_LEN).min(self.scratch.len());
            if self.decode_pos == target_end {
                break;
            }

            let (consumed, produced) = match self.inner.read(
                &input[input_index..],
                &mut self.scratch[..target_end],
                self.decode_pos,
            ) {
                Ok(result) => result,
                Err(error) => {
                    return decompress_failed(ErrorMessage(Some(decompression_message(error))));
                }
            };
            input_index += consumed;
            self.total_in += consumed as u64;
            self.decode_pos += produced;
            self.done = self.inner.is_done();

            // fdeflate may need one more turn after producing output to consume the final EOF
            // marker from its bit buffer. Keep decoding into the contiguous tail until there is
            // no internal progress left, the stream ends, or the staging tail is full.
            if self.done || (consumed == 0 && produced == 0) {
                break;
            }
        }

        self.drain_output(&mut output[output_index..]);

        if self.done && !self.has_pending_output() {
            Ok(Status::StreamEnd)
        } else if before_in == self.total_in && before_out == self.total_out {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }

    fn reset(&mut self, zlib_header: bool) {
        let format = format_from_bool(zlib_header);
        self.inner.reset(format);
        self.drain_pos = 0;
        self.decode_pos = 0;
        self.total_in = 0;
        self.total_out = 0;
        self.done = false;
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
