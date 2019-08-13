//! This module contains backend-specific code.

use mem::{CompressError, DecompressError, FlushCompress, FlushDecompress, Status};
use Compression;

pub use self::imp::*;

/// Traits specifying the interface of the backends.
///
/// Sync + Send are added as a condition to ensure they are available
/// for the frontend.
pub(crate) trait Backend: Sync + Send {
    fn total_in(&self) -> u64;
    fn total_out(&self) -> u64;
}

pub(crate) trait InflateBackend: Backend {
    fn make(zlib_header: bool, window_bits: u8) -> Self;
    fn decompress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushDecompress,
    ) -> Result<Status, DecompressError>;
    fn reset(&mut self, zlib_header: bool);
}

pub(crate) trait DeflateBackend: Backend {
    fn make(level: Compression, zlib_header: bool, window_bits: u8) -> Self;
    fn compress(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: FlushCompress,
    ) -> Result<Status, CompressError>;
    fn reset(&mut self);
}

/// Implementation for C backends.
#[cfg(not(any(
    all(not(feature = "zlib"), feature = "rust_backend"),
    all(target_arch = "wasm32", not(target_os = "emscripten"))
)))]
pub(crate) mod imp {
    use std::{cmp, marker};
    use std::ops::{Deref, DerefMut};

    pub use libc::{c_int, c_uint};

    use super::*;
    use mem::{self, FlushDecompress, Status};

    pub struct StreamWrapper {
        pub(crate) inner: Box<mz_stream>,
    }

    impl ::std::fmt::Debug for StreamWrapper {
        fn fmt(&self, f: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
            write!(f, "StreamWrapper")
        }
    }

    impl Default for StreamWrapper {
        fn default() -> StreamWrapper {
            // Temporary workaround due to bug in libz-sys.
            // Error non-boxed function pointers in data structure),
            // these are not actually used.
            #[allow(unknown_lints)]
            #[allow(invalid_value)]
            StreamWrapper {
                inner: Box::new(unsafe { std::mem::zeroed() }),
            }
        }
    }

    impl Deref for StreamWrapper {
        type Target = mz_stream;

        fn deref(&self) -> &Self::Target {
            &*self.inner
        }
    }

    impl DerefMut for StreamWrapper {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut *self.inner
        }
    }

    unsafe impl<D: Direction> Send for Stream<D> {}
    unsafe impl<D: Direction> Sync for Stream<D> {}

    /// Trait used to call the right destroy/end function on the inner
    /// stream object on drop.
    pub(crate) trait Direction {
        unsafe fn destroy(stream: *mut imp::mz_stream) -> c_int;
    }

    #[derive(Debug)]
    pub(crate) enum DirCompress {}
    #[derive(Debug)]
    pub(crate) enum DirDecompress {}

    #[derive(Debug)]
    pub(crate) struct Stream<D: Direction> {
        pub(crate) stream_wrapper: StreamWrapper,
        pub(crate) total_in: u64,
        pub(crate) total_out: u64,
        pub(crate) _marker: marker::PhantomData<D>,
    }

    impl<D: Direction> Drop for Stream<D> {
        fn drop(&mut self) {
            unsafe {
                let _ = D::destroy(&mut *self.stream_wrapper);
            }
        }
    }

    impl Direction for DirCompress {
        unsafe fn destroy(stream: *mut mz_stream) -> c_int {
            mz_deflateEnd(stream)
        }
    }
    impl Direction for DirDecompress {
        unsafe fn destroy(stream: *mut mz_stream) -> c_int {
            mz_inflateEnd(stream)
        }
    }

    #[derive(Debug)]
    pub(crate) struct CInflate {
        pub(crate) inner: Stream<DirDecompress>,
    }

    impl InflateBackend for CInflate {
        fn make(zlib_header: bool, window_bits: u8) -> Self {
            assert!(
                window_bits > 8 && window_bits < 16,
                "window_bits must be within 9 ..= 15"
            );
            unsafe {
                let mut state = StreamWrapper::default();
                let ret = mz_inflateInit2(
                    &mut *state,
                    if zlib_header {
                        window_bits as c_int
                    } else {
                        -(window_bits as c_int)
                    },
                );
                assert_eq!(ret, 0);
                CInflate {
                    inner: Stream {
                        stream_wrapper: state,
                        total_in: 0,
                        total_out: 0,
                        _marker: marker::PhantomData,
                    },
                }
            }
        }

        fn decompress(
            &mut self,
            input: &[u8],
            output: &mut [u8],
            flush: FlushDecompress,
        ) -> Result<Status, DecompressError> {
            let raw = &mut *self.inner.stream_wrapper;
            raw.next_in = input.as_ptr() as *mut u8;
            raw.avail_in = cmp::min(input.len(), c_uint::max_value() as usize) as c_uint;
            raw.next_out = output.as_mut_ptr();
            raw.avail_out = cmp::min(output.len(), c_uint::max_value() as usize) as c_uint;

            let rc = unsafe { mz_inflate(raw, flush as c_int) };

            // Unfortunately the total counters provided by zlib might be only
            // 32 bits wide and overflow while processing large amounts of data.
            self.inner.total_in += (raw.next_in as usize - input.as_ptr() as usize) as u64;
            self.inner.total_out += (raw.next_out as usize - output.as_ptr() as usize) as u64;

            match rc {
                MZ_DATA_ERROR | MZ_STREAM_ERROR => mem::decompress_failed(),
                MZ_OK => Ok(Status::Ok),
                MZ_BUF_ERROR => Ok(Status::BufError),
                MZ_STREAM_END => Ok(Status::StreamEnd),
                MZ_NEED_DICT => mem::decompress_need_dict(raw.adler as u32),
                c => panic!("unknown return code: {}", c),
            }
        }

        #[cfg(feature = "zlib")]
        fn reset(&mut self, zlib_header: bool) {
            let bits = if zlib_header {
                MZ_DEFAULT_WINDOW_BITS
            } else {
                -MZ_DEFAULT_WINDOW_BITS
            };
            unsafe {
                inflateReset2(&mut *self.inner.stream_wrapper, bits);
            }
            self.inner.total_out = 0;
            self.inner.total_in = 0;
        }

        #[cfg(not(feature = "zlib"))]
        fn reset(&mut self, zlib_header: bool) {
            *self = Self::make(zlib_header, MZ_DEFAULT_WINDOW_BITS as u8);
        }
    }

    impl Backend for CInflate {
        #[inline]
        fn total_in(&self) -> u64 {
            self.inner.total_in
        }

        #[inline]
        fn total_out(&self) -> u64 {
            self.inner.total_out
        }
    }

    #[derive(Debug)]
    pub(crate) struct CDeflate {
        pub(crate) inner: Stream<DirCompress>,
    }

    impl DeflateBackend for CDeflate {
        fn make(level: Compression, zlib_header: bool, window_bits: u8) -> Self {
            assert!(
                window_bits > 8 && window_bits < 16,
                "window_bits must be within 9 ..= 15"
            );
            unsafe {
                let mut state = StreamWrapper::default();
                let ret = mz_deflateInit2(
                    &mut *state,
                    level.0 as c_int,
                    MZ_DEFLATED,
                    if zlib_header {
                        window_bits as c_int
                    } else {
                        -(window_bits as c_int)
                    },
                    9,
                    MZ_DEFAULT_STRATEGY,
                );
                assert_eq!(ret, 0);
                CDeflate {
                    inner: Stream {
                        stream_wrapper: state,
                        total_in: 0,
                        total_out: 0,
                        _marker: marker::PhantomData,
                    },
                }
            }
        }
        fn compress(
            &mut self,
            input: &[u8],
            output: &mut [u8],
            flush: FlushCompress,
        ) -> Result<Status, CompressError> {
            let raw = &mut *self.inner.stream_wrapper;
            raw.next_in = input.as_ptr() as *mut _;
            raw.avail_in = cmp::min(input.len(), c_uint::max_value() as usize) as c_uint;
            raw.next_out = output.as_mut_ptr();
            raw.avail_out = cmp::min(output.len(), c_uint::max_value() as usize) as c_uint;

            let rc = unsafe { mz_deflate(raw, flush as c_int) };

            // Unfortunately the total counters provided by zlib might be only
            // 32 bits wide and overflow while processing large amounts of data.
            self.inner.total_in += (raw.next_in as usize - input.as_ptr() as usize) as u64;
            self.inner.total_out += (raw.next_out as usize - output.as_ptr() as usize) as u64;

            match rc {
                MZ_OK => Ok(Status::Ok),
                MZ_BUF_ERROR => Ok(Status::BufError),
                MZ_STREAM_END => Ok(Status::StreamEnd),
                MZ_STREAM_ERROR => Err(CompressError(())),
                c => panic!("unknown return code: {}", c),
            }
        }

        fn reset(&mut self) {
            self.inner.total_in = 0;
            self.inner.total_out = 0;
            let rc = unsafe { mz_deflateReset(&mut *self.inner.stream_wrapper) };
            assert_eq!(rc, MZ_OK);
        }
    }

    impl Backend for CDeflate {
        #[inline]
        fn total_in(&self) -> u64 {
            self.inner.total_in
        }

        #[inline]
        fn total_out(&self) -> u64 {
            self.inner.total_out
        }
    }

    pub(crate) use self::c_backend::*;

    /// Miniz specific
    #[cfg(not(feature = "zlib"))]
    mod c_backend {
        extern crate miniz_sys;

        pub use self::miniz_sys::*;
    }

    /// Zlib specific
    #[cfg(feature = "zlib")]
    #[allow(bad_style)]
    mod c_backend {
        extern crate libz_sys as z;
        use libc::{c_char, c_int};
        use std::mem;

        pub use self::z::deflate as mz_deflate;
        pub use self::z::deflateEnd as mz_deflateEnd;
        pub use self::z::deflateReset as mz_deflateReset;
        pub use self::z::inflate as mz_inflate;
        pub use self::z::inflateEnd as mz_inflateEnd;
        pub use self::z::z_stream as mz_stream;
        pub use self::z::*;

        pub use self::z::Z_BLOCK as MZ_BLOCK;
        pub use self::z::Z_BUF_ERROR as MZ_BUF_ERROR;
        pub use self::z::Z_DATA_ERROR as MZ_DATA_ERROR;
        pub use self::z::Z_DEFAULT_STRATEGY as MZ_DEFAULT_STRATEGY;
        pub use self::z::Z_DEFLATED as MZ_DEFLATED;
        pub use self::z::Z_FINISH as MZ_FINISH;
        pub use self::z::Z_FULL_FLUSH as MZ_FULL_FLUSH;
        pub use self::z::Z_NEED_DICT as MZ_NEED_DICT;
        pub use self::z::Z_NO_FLUSH as MZ_NO_FLUSH;
        pub use self::z::Z_OK as MZ_OK;
        pub use self::z::Z_PARTIAL_FLUSH as MZ_PARTIAL_FLUSH;
        pub use self::z::Z_STREAM_END as MZ_STREAM_END;
        pub use self::z::Z_STREAM_ERROR as MZ_STREAM_ERROR;
        pub use self::z::Z_SYNC_FLUSH as MZ_SYNC_FLUSH;

        pub const MZ_DEFAULT_WINDOW_BITS: c_int = 15;

        const ZLIB_VERSION: &'static str = "1.2.8\0";

        pub unsafe extern "C" fn mz_deflateInit2(
            stream: *mut mz_stream,
            level: c_int,
            method: c_int,
            window_bits: c_int,
            mem_level: c_int,
            strategy: c_int,
        ) -> c_int {
            z::deflateInit2_(
                stream,
                level,
                method,
                window_bits,
                mem_level,
                strategy,
                ZLIB_VERSION.as_ptr() as *const c_char,
                mem::size_of::<mz_stream>() as c_int,
            )
        }
        pub unsafe extern "C" fn mz_inflateInit2(stream: *mut mz_stream, window_bits: c_int) -> c_int {
            z::inflateInit2_(
                stream,
                window_bits,
                ZLIB_VERSION.as_ptr() as *const c_char,
                mem::size_of::<mz_stream>() as c_int,
            )
        }
    }

    pub(crate) use self::CDeflate as Deflate;
    pub(crate) use self::CInflate as Inflate;
}


/// Implementation for miniz_oxide rust backend.
#[cfg(any(
    all(not(feature = "zlib"), feature = "rust_backend"),
    all(target_arch = "wasm32", not(target_os = "emscripten"))
))]
mod imp {
    extern crate miniz_oxide;

    use std::convert::TryInto;

    use self::miniz_oxide::deflate::core::CompressorOxide;
    use self::miniz_oxide::inflate::stream::InflateState;
    pub use self::miniz_oxide::*;

    pub const MZ_NO_FLUSH: isize = MZFlush::None as isize;
    pub const MZ_PARTIAL_FLUSH: isize = MZFlush::Partial as isize;
    pub const MZ_SYNC_FLUSH: isize = MZFlush::Sync as isize;
    pub const MZ_FULL_FLUSH: isize = MZFlush::Full as isize;
    pub const MZ_FINISH: isize = MZFlush::Finish as isize;

    use super::*;
    use mem;

    fn format_from_bool(zlib_header: bool) -> DataFormat {
        if zlib_header {
            DataFormat::Zlib
        } else {
            DataFormat::Raw
        }
    }

    pub(crate) struct MZOInflate {
        inner: Box<InflateState>,
        total_in: u64,
        total_out: u64,
    }

    impl ::std::fmt::Debug for MZOInflate {
        fn fmt(&self, f: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
            write!(
                f,
                "miniz_oxide inflate internal state. total_in: {}, total_out: {}",
                self.total_in, self.total_out,
            )
        }
    }

    impl InflateBackend for MZOInflate {
        fn make(zlib_header: bool, window_bits: u8) -> Self {
            assert!(
                window_bits > 8 && window_bits < 16,
                "window_bits must be within 9 ..= 15"
            );

            let format = format_from_bool(zlib_header);

            MZOInflate {
                inner: InflateState::new_boxed(format),
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
            let flush = MZFlush::new(flush as i32).unwrap();

            let res = inflate::stream::inflate(&mut self.inner, input, output, flush);
            self.total_in += res.bytes_consumed as u64;
            self.total_out += res.bytes_written as u64;

            match res.status {
                Ok(status) => match status {
                    MZStatus::Ok => Ok(Status::Ok),
                    MZStatus::StreamEnd => Ok(Status::StreamEnd),
                    MZStatus::NeedDict => {
                        mem::decompress_need_dict(self.inner.decompressor().adler32().unwrap_or(0))
                    }
                },
                Err(status) => match status {
                    MZError::Buf => Ok(Status::BufError),
                    _ => mem::decompress_failed(),
                },
            }
        }

        fn reset(&mut self, zlib_header: bool) {
            self.inner.reset(format_from_bool(zlib_header));
            self.total_in = 0;
            self.total_out = 0;
        }
    }

    impl Backend for MZOInflate {
        #[inline]
        fn total_in(&self) -> u64 {
            self.total_in
        }

        #[inline]
        fn total_out(&self) -> u64 {
            self.total_out
        }
    }

    pub(crate) struct MZODeflate {
        inner: Box<CompressorOxide>,
        total_in: u64,
        total_out: u64,
    }

    impl ::std::fmt::Debug for MZODeflate {
        fn fmt(&self, f: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
            write!(
                f,
                "miniz_oxide deflate internal state. total_in: {}, total_out: {}",
                self.total_in, self.total_out,
            )
        }
    }

    impl DeflateBackend for MZODeflate {
        fn make(level: Compression, zlib_header: bool, window_bits: u8) -> Self {
            assert!(
                window_bits > 8 && window_bits < 16,
                "window_bits must be within 9 ..= 15"
            );

            // Check in case the integer value changes at some point.
            debug_assert!(level.level() <= 10);

            let mut inner: Box<CompressorOxide> = Box::default();
            let format = format_from_bool(zlib_header);
            inner.set_format_and_level(format, level.level().try_into().unwrap_or(1));

            MZODeflate {
                inner,
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
            let flush = MZFlush::new(flush as i32).unwrap();
            let res = deflate::stream::deflate(&mut self.inner, input, output, flush);
            self.total_in += res.bytes_consumed as u64;
            self.total_out += res.bytes_written as u64;

            match res.status {
                Ok(status) => match status {
                    MZStatus::Ok => Ok(Status::Ok),
                    MZStatus::StreamEnd => Ok(Status::StreamEnd),
                    MZStatus::NeedDict => Err(CompressError(())),
                },
                Err(status) => match status {
                    MZError::Buf => Ok(Status::BufError),
                    _ => Err(CompressError(())),
                },
            }
        }

        fn reset(&mut self) {
            self.total_in = 0;
            self.total_out = 0;
            self.inner.reset();
        }
    }

    impl Backend for MZODeflate {
        #[inline]
        fn total_in(&self) -> u64 {
            self.total_in
        }

        #[inline]
        fn total_out(&self) -> u64 {
            self.total_out
        }
    }

    pub(crate) use self::MZODeflate as Deflate;
    pub(crate) use self::MZOInflate as Inflate;
}
