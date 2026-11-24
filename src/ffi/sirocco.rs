//! Implementation for the `sirocco` rust backend.
//!
//! sirocco keeps a stream's history in the caller's buffers: the decoder in
//! its output buffer, the encoder in its input buffer. flate2 hands the
//! backend fresh slices on every call, so this adapter owns those buffers,
//! decoding into its own window and copying out, and copying input into its
//! own buffer before compressing from it.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use ::sirocco::deflate::{Encoder, Flush, Level};
use ::sirocco::inflate::{Decoder, Error};
use ::sirocco::Format;

use super::*;
use crate::mem;

pub const MZ_NO_FLUSH: isize = 0;
pub const MZ_PARTIAL_FLUSH: isize = 1;
pub const MZ_SYNC_FLUSH: isize = 2;
pub const MZ_FULL_FLUSH: isize = 3;
pub const MZ_FINISH: isize = 4;
pub const MZ_DEFAULT_WINDOW_BITS: core::ffi::c_int = 15;

/// The largest the adapter's buffers grow to, after which they are recycled
/// with `make_room`. sirocco streams fastest through 256 KiB or more.
const MAX_BUF: usize = 256 << 10;
/// Where the buffers start, so small streams stay small.
const MIN_BUF: usize = 16 << 10;

#[derive(Clone)]
pub struct ErrorMessage(&'static str);

impl ErrorMessage {
    pub fn get(&self) -> Option<&str> {
        Some(self.0)
    }
}

fn format_from_bool(zlib_header: bool) -> Format {
    if zlib_header {
        Format::Zlib
    } else {
        Format::Deflate
    }
}

fn message(e: Error) -> &'static str {
    match e {
        Error::BadData => "invalid compressed data",
        Error::ChecksumMismatch => "checksum mismatch in compressed data",
        Error::Truncated => "compressed data ended early",
        Error::PresetDictionary => "zlib stream needs a preset dictionary",
        _ => "decompression failed",
    }
}

/// Grows `buf` (zero-filled) towards `MAX_BUF` when it has no room left.
/// Returns whether it grew.
fn grow(buf: &mut Vec<u8>) -> bool {
    if buf.len() >= MAX_BUF {
        return false;
    }
    let len = (buf.len() * 2).clamp(MIN_BUF, MAX_BUF);
    buf.resize(len, 0);
    true
}

pub struct Inflate {
    /// Boxed: about 30 KiB, which flate2 would otherwise move around.
    dec: Box<Decoder>,
    /// The decoder's window: everything it has written since the last
    /// `make_room`.
    window: Vec<u8>,
    /// Decoded bytes in `window[pending..pos]` not yet handed out.
    pending: usize,
    /// The end of the decoder's output in `window`.
    pos: usize,
    /// The stream ended; once `pending == pos`, every call is `StreamEnd`.
    done: bool,
    /// A data error, which ends the stream for good, as with zlib.
    failed: Option<Error>,
    /// The stream's first bytes, consumed in calls before this one.
    header: [u8; 6],
    header_len: u8,
    total_in: u64,
    total_out: u64,
}

impl fmt::Debug for Inflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "sirocco inflate internal state. total_in: {}, total_out: {}",
            self.total_in, self.total_out,
        )
    }
}

impl Inflate {
    /// Keeps the stream's first bytes, for the dictionary id in a zlib header.
    fn capture_header(&mut self, consumed: &[u8]) {
        let have = self.header_len as usize;
        if have < self.header.len() {
            let n = consumed.len().min(self.header.len() - have);
            self.header[have..have + n].copy_from_slice(&consumed[..n]);
            self.header_len += n as u8;
        }
    }

    /// The DICTID of a zlib header with FDICT set, from the header bytes
    /// consumed so far followed by the `rest` of the input.
    fn dict_id(&self, rest: &[u8]) -> Option<u32> {
        let have = self.header_len as usize;
        let mut bytes = [0; 6];
        bytes[..have].copy_from_slice(&self.header[..have]);
        let n = rest.len().min(6 - have);
        bytes[have..have + n].copy_from_slice(&rest[..n]);
        (have + n == 6).then(|| u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]))
    }

    /// Decodes a whole stream straight into `output`, skipping the window,
    /// when this call has all of it. Returns false (with nothing changed)
    /// when it doesn't fit or isn't all there.
    fn try_whole(&mut self, input: &[u8], output: &mut [u8]) -> bool {
        match self.dec.decompress(input, output, true) {
            Ok(p) if p.done => {
                self.total_in += p.consumed as u64;
                self.total_out += p.output.len() as u64;
                self.done = true;
                true
            }
            // Out of room or input, or bad data: start over the slow way,
            // which reports errors and partial progress as zlib does.
            _ => {
                self.dec.reset();
                false
            }
        }
    }
}

impl InflateBackend for Inflate {
    fn make(zlib_header: bool, _window_bits: u8) -> Self {
        Inflate {
            dec: Box::new(Decoder::new(format_from_bool(zlib_header))),
            window: Vec::new(),
            pending: 0,
            pos: 0,
            done: false,
            failed: None,
            header: [0; 6],
            header_len: 0,
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
        if let Some(e) = self.failed {
            return mem::decompress_failed(ErrorMessage(message(e)));
        }
        if flush == FlushDecompress::Finish
            && self.total_in == 0
            && !self.done
            && !input.is_empty()
            && !output.is_empty()
            && self.try_whole(input, output)
        {
            return Ok(Status::StreamEnd);
        }

        let (mut in_pos, mut out_pos) = (0, 0);
        loop {
            // Hand out what's decoded first.
            let n = (self.pos - self.pending).min(output.len() - out_pos);
            output[out_pos..out_pos + n]
                .copy_from_slice(&self.window[self.pending..self.pending + n]);
            self.pending += n;
            out_pos += n;
            // With the output full, carry on only while nothing is waiting in the
            // window: the rest of the stream may produce no output, like the
            // final block's end or the zlib trailer.
            if self.done || (out_pos == output.len() && self.pending < self.pos) {
                break;
            }

            if self.pos == self.window.len() && !grow(&mut self.window) {
                let dropped = self.dec.make_room(&mut self.window);
                self.pos -= dropped;
                self.pending -= dropped;
            }
            // Never tell sirocco the input has ended: a cut-off stream is
            // then a `BufError` without progress, which callers check for,
            // rather than an error that ends the stream.
            // sirocco decodes into the window from `pos` on.
            let r = self
                .dec
                .decompress(&input[in_pos..], &mut self.window, false);
            let (consumed, written, done) = match r {
                Ok(p) => (p.consumed, p.output.len(), p.done),
                Err(e) => {
                    self.failed = Some(e);
                    if e == Error::PresetDictionary {
                        if let Some(id) = self.dict_id(&input[in_pos..]) {
                            return mem::decompress_need_dict(id);
                        }
                    }
                    return mem::decompress_failed(ErrorMessage(message(e)));
                }
            };
            self.capture_header(&input[in_pos..in_pos + consumed]);
            in_pos += consumed;
            self.pos += written;
            self.done = done;
            if consumed == 0 && written == 0 && !done {
                // Needs more input.
                break;
            }
        }

        self.total_in += in_pos as u64;
        self.total_out += out_pos as u64;
        if self.done && self.pending == self.pos {
            Ok(Status::StreamEnd)
        } else if in_pos == 0 && out_pos == 0 {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }

    fn reset(&mut self, zlib_header: bool) {
        // Rebuilding the decoder costs about as much as decoding a tiny
        // stream; only do it when the format changes.
        let format = format_from_bool(zlib_header);
        if self.dec.format() == format {
            self.dec.reset();
        } else {
            *self.dec = Decoder::new(format);
        }
        self.pending = 0;
        self.pos = 0;
        self.done = false;
        self.failed = None;
        self.header_len = 0;
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

pub struct Deflate {
    enc: Encoder,
    /// The encoder's input: what it matches against, and what it hasn't
    /// consumed yet.
    buf: Vec<u8>,
    /// The end of the input in `buf`.
    len: usize,
    /// A flush that filled the output; sirocco wants it repeated, with no
    /// new input, until it's out.
    pending_flush: Option<Flush>,
    /// The trailer is written.
    finished: bool,
    total_in: u64,
}

impl fmt::Debug for Deflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "sirocco deflate internal state. total_in: {}, total_out: {}",
            self.total_in,
            self.enc.total_out(),
        )
    }
}

impl From<FlushCompress> for Flush {
    fn from(value: FlushCompress) -> Self {
        match value {
            FlushCompress::None => Flush::None,
            FlushCompress::Partial => Flush::Partial,
            FlushCompress::Sync => Flush::Sync,
            FlushCompress::Full => Flush::Full,
            FlushCompress::Finish => Flush::Finish,
        }
    }
}

impl DeflateBackend for Deflate {
    fn make(level: Compression, zlib_header: bool, _window_bits: u8) -> Self {
        // flate2 allows level 10 (miniz_oxide's "uber" level); sirocco stops at 9.
        let level = Level::new(level.level().min(9) as u8);
        Deflate {
            enc: Encoder::new(format_from_bool(zlib_header), level),
            buf: Vec::new(),
            len: 0,
            pending_flush: None,
            finished: false,
            total_in: 0,
        }
    }

    fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        if self.finished {
            // As zlib: nothing more to say until a reset.
            return Ok(if flush == FlushCompress::Finish && input.is_empty() {
                Status::StreamEnd
            } else {
                Status::BufError
            });
        }
        if self.pending_flush == Some(Flush::Finish) && flush != FlushCompress::Finish {
            // As zlib: a stream being finished can only be finished.
            return mem::compress_failed(ErrorMessage("stream error"));
        }

        let (mut in_pos, mut out_pos) = (0, 0);
        loop {
            // Finish a flush that filled the output before taking more input.
            let pending = self.pending_flush.take();
            if pending.is_none() && in_pos < input.len() {
                if self.len == self.buf.len() && !grow(&mut self.buf) {
                    self.len -= self.enc.make_room(&mut self.buf[..self.len]);
                }
                let n = (input.len() - in_pos).min(self.buf.len() - self.len);
                self.buf[self.len..self.len + n].copy_from_slice(&input[in_pos..in_pos + n]);
                self.len += n;
                in_pos += n;
            }
            // Only flush once all of this call's input is in.
            let f = match pending {
                Some(f) => f,
                None if in_pos < input.len() => Flush::None,
                None => flush.into(),
            };

            let p = self
                .enc
                .compress(&self.buf[..self.len], &mut output[out_pos..], f);
            let (written, full, done) = (p.output.len(), p.full, p.done);
            out_pos += written;
            if done {
                self.finished = true;
                break;
            }
            if full {
                if f != Flush::None {
                    self.pending_flush = Some(f);
                }
                break;
            }
            if pending.is_some() {
                // The old flush is out; carry on with this call's request.
                continue;
            }
            if in_pos == input.len() || out_pos == output.len() {
                break;
            }
        }

        self.total_in += in_pos as u64;
        if self.finished {
            Ok(Status::StreamEnd)
        } else if in_pos == 0 && out_pos == 0 {
            Ok(Status::BufError)
        } else {
            Ok(Status::Ok)
        }
    }

    fn reset(&mut self) {
        self.enc.reset();
        self.len = 0;
        self.pending_flush = None;
        self.finished = false;
        self.total_in = 0;
    }
}

impl Backend for Deflate {
    #[inline]
    fn total_in(&self) -> u64 {
        self.total_in
    }

    #[inline]
    fn total_out(&self) -> u64 {
        self.enc.total_out()
    }
}
