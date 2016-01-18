pub use self::imp::*;

#[cfg(feature = "zlib")]
#[allow(bad_style)]
mod imp {
    extern crate libz_sys as z;
    use std::mem;
    use libc::{c_int, size_t, c_ulong, c_uint, c_char};

    pub use self::z::deflateEnd as mz_deflateEnd;
    pub use self::z::inflateEnd as mz_inflateEnd;
    pub use self::z::deflateReset as mz_deflateReset;
    pub use self::z::deflate as mz_deflate;
    pub use self::z::inflate as mz_inflate;
    pub use self::z::z_stream as mz_stream;

    pub use self::z::Z_BLOCK as MZ_BLOCK;
    pub use self::z::Z_BUF_ERROR as MZ_BUF_ERROR;
    pub use self::z::Z_DATA_ERROR as MZ_DATA_ERROR;
    pub use self::z::Z_DEFAULT_STRATEGY as MZ_DEFAULT_STRATEGY;
    pub use self::z::Z_DEFLATED as MZ_DEFLATED;
    pub use self::z::Z_FINISH as MZ_FINISH;
    pub use self::z::Z_FULL_FLUSH as MZ_FULL_FLUSH;
    pub use self::z::Z_NO_FLUSH as MZ_NO_FLUSH;
    pub use self::z::Z_OK as MZ_OK;
    pub use self::z::Z_PARTIAL_FLUSH as MZ_PARTIAL_FLUSH;
    pub use self::z::Z_STREAM_END as MZ_STREAM_END;
    pub use self::z::Z_SYNC_FLUSH as MZ_SYNC_FLUSH;

    pub const MZ_DEFAULT_WINDOW_BITS: c_int = 15;

    pub unsafe extern fn mz_crc32(crc: c_ulong,
                                  ptr: *const u8,
                                  len: size_t) -> c_ulong {
        z::crc32(crc, ptr, len as c_uint)
    }

    const ZLIB_VERSION: &'static str = "1.2.8\0";

    pub unsafe extern fn mz_deflateInit2(stream: *mut mz_stream,
                                         level: c_int,
                                         method: c_int,
                                         window_bits: c_int,
                                         mem_level: c_int,
                                         strategy: c_int) -> c_int {
        z::deflateInit2_(stream, level, method, window_bits, mem_level,
                         strategy,
                         ZLIB_VERSION.as_ptr() as *const c_char,
                         mem::size_of::<mz_stream>() as c_int)
    }
    pub unsafe extern fn mz_inflateInit2(stream: *mut mz_stream,
                                         window_bits: c_int)
                                         -> c_int {
        z::inflateInit2_(stream, window_bits,
                         ZLIB_VERSION.as_ptr() as *const c_char,
                         mem::size_of::<mz_stream>() as c_int)
    }
}

#[cfg(not(feature = "zlib"))]
mod imp {
    extern crate miniz_sys;

    pub use self::miniz_sys::*;
}
