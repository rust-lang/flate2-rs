//! Implementation for `miniz_oxide` rust backend.

use std::fmt;

use ::zlib_rs::{DeflateFlush, InflateFlush};

pub const MZ_NO_FLUSH: isize = DeflateFlush::NoFlush as isize;
pub const MZ_PARTIAL_FLUSH: isize = DeflateFlush::PartialFlush as isize;
pub const MZ_SYNC_FLUSH: isize = DeflateFlush::SyncFlush as isize;
pub const MZ_FULL_FLUSH: isize = DeflateFlush::FullFlush as isize;
pub const MZ_FINISH: isize = DeflateFlush::Finish as isize;

pub const MZ_DEFAULT_WINDOW_BITS: core::ffi::c_int = 15;

use super::*;

#[derive(Clone, Default)]
pub struct ErrorMessage(Option<&'static str>);

impl ErrorMessage {
    pub fn get(&self) -> Option<&str> {
        self.0
    }
}

pub struct Inflate {
    pub(crate) inner: api::Decompress,
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
            inner: api::Decompress::new(zlib_header, window_bits),
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

        self.inner.decompress(input, output, flush)
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
    pub(crate) inner: api::Compress,
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
            inner: api::Compress::new(level.level() as i32, zlib_header, window_bits),
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
        self.inner.compress(input, output, flush)
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

mod api {
    use ::zlib_rs::ReturnCode;
    use ::zlib_rs::{DeflateFlush, InflateFlush};

    use super::MZ_DEFAULT_WINDOW_BITS;

    // FIXME: zlib_rs should have structured error types.
    use super::{CompressError, DecompressError, ErrorMessage, Status};

    /// A type to hold all state needed for decompressing a ZLIB encoded stream.
    pub struct Decompress(libz_rs_sys::z_stream);

    unsafe impl Sync for Decompress {}
    unsafe impl Send for Decompress {}

    impl Decompress {
        /// The amount of bytes consumed from the input so far.
        pub fn total_in(&self) -> u64 {
            self.0.total_in as _
        }

        /// The amount of decompressed bytes that have been written to the output thus far.
        pub fn total_out(&self) -> u64 {
            self.0.total_out as _
        }

        /// Create a new instance. Note that it allocates in various ways and thus should be re-used.
        pub fn new(zlib_header: bool, window_bits: u8) -> Self {
            let mut this = libz_rs_sys::z_stream::default();

            unsafe {
                libz_rs_sys::inflateInit2_(
                    &mut this,
                    if zlib_header {
                        i32::from(window_bits)
                    } else {
                        -i32::from(window_bits)
                    },
                    libz_rs_sys::zlibVersion(),
                    core::mem::size_of::<libz_rs_sys::z_stream>() as core::ffi::c_int,
                );
            }

            Self(this)
        }

        /// Reset the state to allow handling a new stream.
        pub fn reset(&mut self, zlib_header: bool) {
            let bits = if zlib_header {
                MZ_DEFAULT_WINDOW_BITS
            } else {
                -MZ_DEFAULT_WINDOW_BITS
            };
            unsafe { libz_rs_sys::inflateReset2(&mut self.0, bits) };
        }

        /// Decompress `input` and write all decompressed bytes into `output`, with `flush` defining some details about this.
        pub fn decompress(
            &mut self,
            input: &[u8],
            output: &mut [u8],
            flush: InflateFlush,
        ) -> Result<Status, DecompressError> {
            self.0.avail_in = input.len() as _;
            self.0.avail_out = output.len() as _;

            self.0.next_in = input.as_ptr();
            self.0.next_out = output.as_mut_ptr();

            match unsafe { libz_rs_sys::inflate(&mut self.0, flush as _) } {
                libz_rs_sys::Z_OK => Ok(Status::Ok),
                libz_rs_sys::Z_BUF_ERROR => Ok(Status::BufError),
                libz_rs_sys::Z_STREAM_END => Ok(Status::StreamEnd),
                libz_rs_sys::Z_NEED_DICT => crate::mem::decompress_need_dict(self.0.adler as u32),

                err => {
                    let msg = match ReturnCode::try_from_c_int(err) {
                        None => None,
                        Some(code) => {
                            let s = unsafe { std::ffi::CStr::from_ptr(code.error_message()) };
                            std::str::from_utf8(s.to_bytes()).ok()
                        }
                    };

                    crate::mem::decompress_failed(ErrorMessage(msg))
                }
            }
        }
    }

    impl Drop for Decompress {
        fn drop(&mut self) {
            unsafe { libz_rs_sys::inflateEnd(&mut self.0) };
        }
    }

    /// Hold all state needed for compressing data.
    pub struct Compress(libz_rs_sys::z_stream);

    unsafe impl Sync for Compress {}
    unsafe impl Send for Compress {}

    impl Compress {
        /// The number of bytes that were read from the input.
        pub fn total_in(&self) -> u64 {
            self.0.total_in as _
        }

        /// The number of compressed bytes that were written to the output.
        pub fn total_out(&self) -> u64 {
            self.0.total_out as _
        }

        /// Create a new instance - this allocates so should be done with care.
        pub fn new(level: i32, zlib_header: bool, window_bits: u8) -> Self {
            // Check in case the integer value changes at some point.
            debug_assert!(level <= 9);

            let mut this = libz_rs_sys::z_stream::default();

            unsafe {
                libz_rs_sys::deflateInit2_(
                    &mut this,
                    level,
                    ::zlib_rs::deflate::Method::default() as i32,
                    if zlib_header {
                        window_bits as i32
                    } else {
                        -(window_bits as i32)
                    },
                    8,
                    ::zlib_rs::deflate::Strategy::default() as i32,
                    libz_rs_sys::zlibVersion(),
                    core::mem::size_of::<libz_rs_sys::z_stream>() as core::ffi::c_int,
                );
            }

            Self(this)
        }

        /// Prepare the instance for a new stream.
        pub fn reset(&mut self) {
            unsafe { libz_rs_sys::deflateReset(&mut self.0) };
        }

        /// Compress `input` and write compressed bytes to `output`, with `flush` controlling additional characteristics.
        pub fn compress(
            &mut self,
            input: &[u8],
            output: &mut [u8],
            flush: DeflateFlush,
        ) -> Result<Status, CompressError> {
            self.0.avail_in = input.len() as _;
            self.0.avail_out = output.len() as _;

            self.0.next_in = input.as_ptr();
            self.0.next_out = output.as_mut_ptr();

            match unsafe { libz_rs_sys::deflate(&mut self.0, flush as _) } {
                libz_rs_sys::Z_OK => Ok(Status::Ok),
                libz_rs_sys::Z_BUF_ERROR => Ok(Status::BufError),
                libz_rs_sys::Z_STREAM_END => Ok(Status::StreamEnd),

                err => {
                    let msg = match ReturnCode::try_from_c_int(err) {
                        None => None,
                        Some(code) => {
                            let s = unsafe { std::ffi::CStr::from_ptr(code.error_message()) };
                            std::str::from_utf8(s.to_bytes()).ok()
                        }
                    };

                    crate::mem::compress_failed(ErrorMessage(msg))
                }
            }
        }
    }

    impl Drop for Compress {
        fn drop(&mut self) {
            unsafe { libz_rs_sys::deflateEnd(&mut self.0) };
        }
    }
}
