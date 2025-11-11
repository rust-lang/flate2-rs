//! Implementation for `miniz_oxide` rust backend.

use std::fmt;

use ::zlib_rs::{Compress, Decompress, DeflateFlush, InflateFlush};

pub const MZ_NO_FLUSH: isize = DeflateFlush::NoFlush as isize;
pub const MZ_PARTIAL_FLUSH: isize = DeflateFlush::PartialFlush as isize;
pub const MZ_SYNC_FLUSH: isize = DeflateFlush::SyncFlush as isize;
pub const MZ_FULL_FLUSH: isize = DeflateFlush::FullFlush as isize;
pub const MZ_FINISH: isize = DeflateFlush::Finish as isize;

pub const MZ_DEFAULT_WINDOW_BITS: core::ffi::c_int = 15;

use super::*;

impl From<::zlib_rs::Status> for crate::mem::Status {
    fn from(value: ::zlib_rs::Status) -> Self {
        match value {
            ::zlib_rs::Status::Ok => crate::mem::Status::Ok,
            ::zlib_rs::Status::BufError => crate::mem::Status::BufError,
            ::zlib_rs::Status::StreamEnd => crate::mem::Status::StreamEnd,
        }
    }
}

#[derive(Clone, Default)]
pub struct ErrorMessage(Option<&'static str>);

impl ErrorMessage {
    pub fn get(&self) -> Option<&str> {
        self.0
    }
}

pub struct Inflate {
    pub(crate) inner: Decompress,
}

impl fmt::Debug for Inflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "miniz_oxide inflate internal state. total_in: {}, total_out: {}",
            self.inner.total_in(),
            self.inner.total_out(),
        )
    }
}

impl From<FlushDecompress> for DeflateFlush {
    fn from(value: FlushDecompress) -> Self {
        match value {
            FlushDecompress::None => Self::NoFlush,
            FlushDecompress::Sync => Self::SyncFlush,
            FlushDecompress::Finish => Self::Finish,
        }
    }
}

impl InflateBackend for Inflate {
    fn make(zlib_header: bool, window_bits: u8) -> Self {
        Inflate {
            inner: Decompress::new(zlib_header, window_bits),
        }
    }

    fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError> {
        let flush = match flush {
            FlushDecompress::None => InflateFlush::NoFlush,
            FlushDecompress::Sync => InflateFlush::SyncFlush,
            FlushDecompress::Finish => InflateFlush::Finish,
        };

        match self.inner.decompress(input, output, flush) {
            Ok(status) => Ok(status.into()),
            Err(::zlib_rs::DecompressError::NeedDict { dict_id }) => {
                crate::mem::decompress_need_dict(dict_id)
            }
            Err(e) => crate::mem::decompress_failed(ErrorMessage(Some(e.as_str()))),
        }
    }

    fn reset(&mut self, zlib_header: bool) {
        self.inner.reset(zlib_header);
    }
}

impl Backend for Inflate {
    #[inline]
    fn total_in(&self) -> u64 {
        self.inner.total_in()
    }

    #[inline]
    fn total_out(&self) -> u64 {
        self.inner.total_out()
    }
}

pub struct Deflate {
    pub(crate) inner: Compress,
}

impl fmt::Debug for Deflate {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "zlib_rs deflate internal state. total_in: {}, total_out: {}",
            self.inner.total_in(),
            self.inner.total_out(),
        )
    }
}

impl DeflateBackend for Deflate {
    fn make(level: Compression, zlib_header: bool, window_bits: u8) -> Self {
        // Check in case the integer value changes at some point.
        debug_assert!(level.level() <= 9);

        Deflate {
            inner: Compress::new(level.level() as i32, zlib_header, window_bits),
        }
    }

    fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushCompress,
    ) -> Result<Status, CompressError> {
        let flush = match flush {
            FlushCompress::None => DeflateFlush::NoFlush,
            FlushCompress::Partial => DeflateFlush::PartialFlush,
            FlushCompress::Sync => DeflateFlush::SyncFlush,
            FlushCompress::Full => DeflateFlush::FullFlush,
            FlushCompress::Finish => DeflateFlush::Finish,
        };

        match self.inner.compress(input, output, flush) {
            Ok(status) => Ok(status.into()),
            Err(e) => crate::mem::compress_failed(ErrorMessage(Some(e.as_str()))),
        }
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl Backend for Deflate {
    #[inline]
    fn total_in(&self) -> u64 {
        self.inner.total_in()
    }

    #[inline]
    fn total_out(&self) -> u64 {
        self.inner.total_out()
    }
}
