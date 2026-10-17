//! Implementation for the `fdeflate` Rust backend.
#![forbid(unsafe_code)]

use super::*;
use crate::mem::{compress_failed, decompress_failed};
use ::fdeflate::{
    CompressionFlush, CompressionProgress, CompressionStatus, DecompressionError as FdeflateError,
    DecompressionStatus,
};
use alloc::vec::Vec;
use core::{fmt, mem::MaybeUninit};

pub const MZ_NO_FLUSH: isize = 0;
pub const MZ_PARTIAL_FLUSH: isize = 1;
pub const MZ_SYNC_FLUSH: isize = 2;
pub const MZ_FULL_FLUSH: isize = 3;
pub const MZ_FINISH: isize = 4;
pub const MZ_DEFAULT_WINDOW_BITS: core::ffi::c_int = 15;

const HISTORY: usize = 32 * 1024;
const TAIL: usize = 64 * 1024;
const SCRATCH: usize = HISTORY + TAIL;

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

fn compression_flush(flush: FlushCompress) -> CompressionFlush {
    match flush {
        FlushCompress::None => CompressionFlush::None,
        FlushCompress::Partial => CompressionFlush::Partial,
        FlushCompress::Sync => CompressionFlush::Sync,
        FlushCompress::Full => CompressionFlush::Full,
        FlushCompress::Finish => CompressionFlush::Finish,
    }
}

pub struct Deflate {
    inner: ::fdeflate::StreamingCompressor,
    zlib_header: bool,
    total_in: u64,
    total_out: u64,
}
impl fmt::Debug for Deflate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("fdeflate::Deflate")
            .field("total_in", &self.total_in)
            .field("total_out", &self.total_out)
            .finish()
    }
}
impl Deflate {
    fn account(&mut self, progress: CompressionProgress) -> Result<Status, CompressError> {
        self.total_in += progress.consumed as u64;
        self.total_out += progress.produced as u64;
        match progress.status {
            Err(_) => compress_failed(ErrorMessage(Some(
                "input supplied after finishing compression",
            ))),
            Ok(CompressionStatus::StreamEnd) => Ok(Status::StreamEnd),
            Ok(_) if progress.consumed == 0 && progress.produced == 0 => Ok(Status::BufError),
            Ok(_) => Ok(Status::Ok),
        }
    }
}
impl DeflateBackend for Deflate {
    fn make(level: Compression, zlib_header: bool, _window_bits: u8) -> Self {
        Self {
            inner: ::fdeflate::StreamingCompressor::new(level.level().min(9) as u8, zlib_header),
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
        let progress = self.inner.process(input, output, compression_flush(flush));
        self.account(progress)
    }
    fn compress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        let progress = self
            .inner
            .process_uninit(input, output, compression_flush(flush));
        self.account(progress)
    }
    fn reset(&mut self) {
        self.inner.reset(self.zlib_header);
        self.total_in = 0;
        self.total_out = 0;
    }
}
impl Backend for Deflate {
    fn total_in(&self) -> u64 {
        self.total_in
    }
    fn total_out(&self) -> u64 {
        self.total_out
    }
}

trait Output {
    fn len(&self) -> usize;
    fn put(&mut self, offset: usize, bytes: &[u8]);
    fn initialized(&mut self) -> Option<&mut [u8]> {
        None
    }
}
impl Output for [u8] {
    fn len(&self) -> usize {
        self.len()
    }
    fn put(&mut self, offset: usize, bytes: &[u8]) {
        self[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
    fn initialized(&mut self) -> Option<&mut [u8]> {
        Some(self)
    }
}
impl Output for [MaybeUninit<u8>] {
    fn len(&self) -> usize {
        self.len()
    }
    fn put(&mut self, offset: usize, bytes: &[u8]) {
        // .write_copy_of_slice() has MSRV of 1.93, we can't use it yet
        for (destination, &byte) in self[offset..offset + bytes.len()].iter_mut().zip(bytes) {
            destination.write(byte);
        }
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
    error: Option<&'static str>,
}
impl fmt::Debug for Inflate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("fdeflate::Inflate")
            .field("drain_pos", &self.drain_pos)
            .field("decode_pos", &self.decode_pos)
            .field("total_in", &self.total_in)
            .field("total_out", &self.total_out)
            .field("done", &self.done)
            .field("error", &self.error)
            .finish()
    }
}
impl Inflate {
    fn drain<O: Output + ?Sized>(&mut self, output: &mut O, offset: &mut usize) {
        let n = (self.decode_pos - self.drain_pos).min(output.len() - *offset);
        if n != 0 {
            output.put(*offset, &self.scratch[self.drain_pos..self.drain_pos + n]);
        }
        self.drain_pos += n;
        *offset += n;
        self.total_out += n as u64;
    }

    fn prepare_scratch(&mut self, writable: usize) {
        if self.scratch.is_empty() {
            self.scratch.resize(SCRATCH, 0);
        }
        if self.decode_pos + writable > SCRATCH {
            let keep = self.decode_pos.min(HISTORY);
            self.scratch
                .copy_within(self.decode_pos - keep..self.decode_pos, 0);
            self.decode_pos = keep;
            self.drain_pos = keep;
        }
    }

    fn run<O: Output + ?Sized>(
        &mut self,
        input: &[u8],
        output: &mut O,
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        if let Some(error) = self.error {
            return decompress_failed(ErrorMessage(Some(error)));
        }
        let before = (self.total_in, self.total_out);
        let mut input_pos = 0;
        let mut output_pos = 0;
        loop {
            self.drain(output, &mut output_pos);
            if self.drain_pos != self.decode_pos || self.done {
                break;
            }

            // The current call's output can replace scratch history once it
            // contains a whole window. A fresh stream needs no history at all.
            if (self.decode_pos == 0 || output_pos >= HISTORY) && output.len() - output_pos >= TAIL
            {
                if let Some(destination) = output.initialized() {
                    let progress =
                        self.inner
                            .read_progress(&input[input_pos..], destination, output_pos);
                    input_pos += progress.consumed;
                    self.total_in += progress.consumed as u64;
                    output_pos += progress.produced;
                    self.total_out += progress.produced as u64;
                    match progress.status {
                        Err(error) => {
                            let message = decompression_message(error);
                            self.error = Some(message);
                            return decompress_failed(ErrorMessage(Some(message)));
                        }
                        Ok(status) => {
                            self.done = status == DecompressionStatus::Done;
                            if !self.done {
                                let keep = output_pos.min(HISTORY);
                                self.prepare_scratch(0);
                                self.scratch[..keep]
                                    .copy_from_slice(&destination[output_pos - keep..output_pos]);
                                self.decode_pos = keep;
                                self.drain_pos = keep;
                            }
                            if self.done || status == DecompressionStatus::NeedsInput {
                                break;
                            }
                            // A full destination may still admit non-producing
                            // headers/trailers through the scratch path below.
                            continue;
                        }
                    }
                }
            }

            let remaining = output.len() - output_pos;
            let writable = if remaining == 0 {
                0
            } else if output.len() >= SCRATCH && output_pos < HISTORY {
                // Establish history in a large destination, then decode into it.
                HISTORY - output_pos
            } else {
                TAIL
            };
            self.prepare_scratch(writable);
            let progress = self.inner.read_progress(
                &input[input_pos..],
                &mut self.scratch[..self.decode_pos + writable],
                self.decode_pos,
            );
            input_pos += progress.consumed;
            self.total_in += progress.consumed as u64;
            self.decode_pos += progress.produced;
            self.drain(output, &mut output_pos);
            match progress.status {
                Err(error) => {
                    let message = decompression_message(error);
                    self.error = Some(message);
                    return decompress_failed(ErrorMessage(Some(message)));
                }
                Ok(status) => {
                    self.done = status == DecompressionStatus::Done;
                    if self.done
                        || status == DecompressionStatus::NeedsInput
                        || (progress.consumed == 0 && progress.produced == 0)
                    {
                        break;
                    }
                }
            }
        }
        if self.done && self.drain_pos == self.decode_pos {
            Ok(Status::StreamEnd)
        } else if flush == FlushDecompress::Finish || before == (self.total_in, self.total_out) {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }
}
impl InflateBackend for Inflate {
    fn make(zlib_header: bool, _window_bits: u8) -> Self {
        Self {
            inner: ::fdeflate::Decompressor::new_with_format(format_from_bool(zlib_header)),
            scratch: Vec::new(),
            drain_pos: 0,
            decode_pos: 0,
            total_in: 0,
            total_out: 0,
            done: false,
            error: None,
        }
    }
    fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        self.run(input, output, flush)
    }
    fn decompress_uninit(
        &mut self,
        input: &[u8],
        output: &mut [MaybeUninit<u8>],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        self.run(input, output, flush)
    }
    fn reset(&mut self, zlib_header: bool) {
        self.inner.reset(format_from_bool(zlib_header));
        self.drain_pos = 0;
        self.decode_pos = 0;
        self.total_in = 0;
        self.total_out = 0;
        self.done = false;
        self.error = None;
    }
}
impl Backend for Inflate {
    fn total_in(&self) -> u64 {
        self.total_in
    }
    fn total_out(&self) -> u64 {
        self.total_out
    }
}
