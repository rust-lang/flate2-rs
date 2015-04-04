//! Raw un-exported bindings to miniz for encoding/decoding

use std::marker;
use std::mem;
use libc::{c_int, c_uint};

use Compression;
use ffi;

pub struct Stream<D: Direction> {
    raw: ffi::mz_stream,
    _marker: marker::PhantomData<D>,
}

pub enum Compress {}
pub enum Decompress {}

pub enum Flush {
    None = ffi::MZ_NO_FLUSH as isize,
    Sync = ffi::MZ_SYNC_FLUSH as isize,
    Finish = ffi::MZ_FINISH as isize,
}

#[doc(hidden)]
pub trait Direction {
    unsafe fn destroy(stream: *mut ffi::mz_stream) -> c_int;
    fn foo(&self) {}
}

impl Stream<Compress> {
    pub fn new_compress(lvl: Compression, raw: bool) -> Stream<Compress> {
        unsafe {
            let mut state: ffi::mz_stream = mem::zeroed();
            let ret = ffi::mz_deflateInit2(&mut state,
                                           lvl as c_int,
                                           ffi::MZ_DEFLATED,
                                           if raw {
                                               -ffi::MZ_DEFAULT_WINDOW_BITS
                                           } else {
                                               ffi::MZ_DEFAULT_WINDOW_BITS
                                           },
                                           9,
                                           ffi::MZ_DEFAULT_STRATEGY);
            debug_assert_eq!(ret, 0);
            Stream { raw: state, _marker: marker::PhantomData }
        }
    }

    pub fn new_decompress(raw: bool) -> Stream<Decompress> {
        unsafe {
            let mut state: ffi::mz_stream = mem::zeroed();
            let ret = ffi::mz_inflateInit2(&mut state,
                                          if raw {
                                              -ffi::MZ_DEFAULT_WINDOW_BITS
                                          } else {
                                              ffi::MZ_DEFAULT_WINDOW_BITS
                                          });
            debug_assert_eq!(ret, 0);
            Stream { raw: state, _marker: marker::PhantomData }
        }
    }
}

impl<T: Direction> Stream<T> {
    pub fn total_in(&self) -> u64 { self.raw.total_in as u64 }
    pub fn total_out(&self) -> u64 { self.raw.total_out as u64 }
}

impl Stream<Decompress> {
    pub fn decompress(&mut self, input: &[u8], output: &mut [u8],
                      flush: Flush) -> c_int {
        self.raw.next_in = input.as_ptr();
        self.raw.avail_in = input.len() as c_uint;
        self.raw.next_out = output.as_mut_ptr();
        self.raw.avail_out = output.len() as c_uint;
        unsafe { ffi::mz_inflate(&mut self.raw, flush as c_int) }
    }

    pub fn decompress_vec(&mut self, input: &[u8], output: &mut Vec<u8>,
                          flush: Flush) -> c_int {
        let cap = output.capacity();
        let len = output.len();
        self.raw.avail_in = input.len() as c_uint;
        self.raw.next_in = input.as_ptr() as *mut _;
        self.raw.avail_out = (cap - len) as c_uint;

        unsafe {
            self.raw.next_out = output.as_mut_ptr().offset(len as isize);
            let before = self.total_out();
            let rc = ffi::mz_inflate(&mut self.raw, flush as c_int);
            let diff = (self.total_out() - before) as usize;
            output.set_len(len + diff);
            return rc;
        }
    }
}

impl Stream<Compress> {
    pub fn compress(&mut self, input: &[u8], output: &mut [u8],
                    flush: Flush) -> c_int {
        self.raw.next_in = input.as_ptr() as *mut _;
        self.raw.avail_in = input.len() as c_uint;
        self.raw.next_out = output.as_mut_ptr();
        self.raw.avail_out = output.len() as c_uint;
        unsafe { ffi::mz_deflate(&mut self.raw, flush as c_int) }
    }

    pub fn compress_vec(&mut self, input: &[u8], output: &mut Vec<u8>,
                        flush: Flush) -> c_int {
        let cap = output.capacity();
        let len = output.len();
        self.raw.avail_in = input.len() as c_uint;
        self.raw.next_in = input.as_ptr() as *mut _;
        self.raw.avail_out = (cap - len) as c_uint;

        unsafe {
            self.raw.next_out = output.as_mut_ptr().offset(len as isize);

            let before = self.total_out();
            let rc = ffi::mz_deflate(&mut self.raw, flush as c_int);
            let diff = (self.total_out() - before) as usize;
            output.set_len(len + diff);
            return rc;
        }
    }
}

impl Direction for Compress {
    unsafe fn destroy(stream: *mut ffi::mz_stream) -> c_int {
        ffi::mz_deflateEnd(stream)
    }
}
impl Direction for Decompress {
    unsafe fn destroy(stream: *mut ffi::mz_stream) -> c_int {
        ffi::mz_inflateEnd(stream)
    }
}

impl<D: Direction> Drop for Stream<D> {
    fn drop(&mut self) {
        unsafe {
            let _ = <D as Direction>::destroy(&mut self.raw);
        }
    }
}
