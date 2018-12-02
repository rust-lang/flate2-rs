pub use self::imp::*;

#[cfg(feature = "zlib")]
#[allow(bad_style)]
mod imp {
    extern crate libz_sys as z;
    use std::mem;
    use std::ops::{Deref, DerefMut};
    use libc::{c_char, c_int};

    pub use self::z::*;
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
    pub use self::z::Z_STREAM_ERROR as MZ_STREAM_ERROR;
    pub use self::z::Z_NEED_DICT as MZ_NEED_DICT;

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

    pub struct StreamWrapper {
        inner: Box<mz_stream>,
    }

    impl ::std::fmt::Debug for StreamWrapper {
        fn fmt(&self, f: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
            write!(f, "StreamWrapper")
        }
    }

    impl Default for StreamWrapper {
        fn default() -> StreamWrapper {
            StreamWrapper {
                inner: Box::new(unsafe { mem::zeroed() }),
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
}

#[cfg(any(all(not(feature = "zlib"), feature = "rust_backend"), all(target_arch = "wasm32", not(target_os = "emscripten"))))]
mod imp {
    extern crate miniz_oxide_c_api;
    use std::ops::{Deref, DerefMut};

    pub use self::miniz_oxide_c_api::*;
    pub use self::miniz_oxide_c_api::lib_oxide::*;

    #[derive(Debug, Default)]
    pub struct StreamWrapper {
        inner: mz_stream,
    }

    impl Deref for StreamWrapper {
        type Target = mz_stream;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl DerefMut for StreamWrapper {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }
}

#[cfg(all(not(feature = "zlib"), not(feature = "rust_backend"), not(all(target_arch = "wasm32", not(target_os = "emscripten")))))]
mod imp {
    extern crate miniz_sys;
    use std::mem;
    use std::ops::{Deref, DerefMut};

    pub use self::miniz_sys::*;

    pub struct StreamWrapper {
        inner: mz_stream,
    }

    impl ::std::fmt::Debug for StreamWrapper {
        fn fmt(&self, f: &mut ::std::fmt::Formatter) -> Result<(), ::std::fmt::Error> {
            write!(f, "StreamWrapper")
        }
    }

    impl Default for StreamWrapper {
        fn default() -> StreamWrapper {
            StreamWrapper {
                inner: unsafe { mem::zeroed() },
            }
        }
    }

    impl Deref for StreamWrapper {
        type Target = mz_stream;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl DerefMut for StreamWrapper {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }
}

