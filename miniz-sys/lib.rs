#![doc(html_root_url = "http://alexcrichton.com/flate2-rs")]
#![allow(bad_style)]

extern crate libc;

pub const MZ_NO_FLUSH: libc::c_int = 0;
pub const MZ_SYNC_FLUSH: libc::c_int = 2;
pub const MZ_FINISH: libc::c_int = 4;

pub const MZ_OK: libc::c_int = 0;
pub const MZ_STREAM_END: libc::c_int = 1;
pub const MZ_NEED_DICT: libc::c_int = 2;
pub const MZ_ERRNO: libc::c_int = -1;
pub const MZ_STREAM_ERROR: libc::c_int = -2;
pub const MZ_DATA_ERROR: libc::c_int = -3;
pub const MZ_MEM_ERROR: libc::c_int = -4;
pub const MZ_BUF_ERROR: libc::c_int = -5;
pub const MZ_VERSION_ERROR: libc::c_int = -6;
pub const MZ_PARAM_ERROR: libc::c_int = -10000;

pub const MZ_DEFLATED: libc::c_int = 8;
pub const MZ_DEFAULT_WINDOW_BITS: libc::c_int = 15;
pub const MZ_DEFAULT_STRATEGY: libc::c_int = 0;

#[repr(C)]
pub struct mz_stream {
    pub next_in: *const u8,
    pub avail_in: libc::c_uint,
    pub total_in: libc::c_ulong,

    pub next_out: *mut u8,
    pub avail_out: libc::c_uint,
    pub total_out: libc::c_ulong,

    pub msg: *const libc::c_char,
    pub state: *mut mz_internal_state,

    pub zalloc: Option<mz_alloc_func>,
    pub zfree: Option<mz_free_func>,
    pub opaque: *mut libc::c_void,

    pub data_type: libc::c_int,
    pub adler: libc::c_ulong,
    pub reserved: libc::c_ulong,
}

pub enum mz_internal_state {}

pub type mz_alloc_func = extern fn(*mut libc::c_void,
                                   libc::size_t,
                                   libc::size_t) -> *mut libc::c_void;
pub type mz_free_func = extern fn(*mut libc::c_void, *mut libc::c_void);

extern {
    pub fn mz_deflateInit2(stream: *mut mz_stream,
                           level: libc::c_int,
                           method: libc::c_int,
                           window_bits: libc::c_int,
                           mem_level: libc::c_int,
                           strategy: libc::c_int) -> libc::c_int;
    pub fn mz_deflate(stream: *mut mz_stream, flush: libc::c_int) -> libc::c_int;
    pub fn mz_deflateEnd(stream: *mut mz_stream) -> libc::c_int;

    pub fn mz_inflateInit2(stream: *mut mz_stream,
                           window_bits: libc::c_int) -> libc::c_int;
    pub fn mz_inflate(stream: *mut mz_stream, flush: libc::c_int) -> libc::c_int;
    pub fn mz_inflateEnd(stream: *mut mz_stream) -> libc::c_int;

    pub fn mz_crc32(crc: libc::c_ulong, ptr: *const u8,
                    len: libc::size_t) -> libc::c_ulong;
}

