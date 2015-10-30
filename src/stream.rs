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

/// Values which indicate the form of flushing to be used when compressing or
/// decompressing in-memory data.
pub enum Flush {
    /// A typical parameter for passing to compression/decompression functions,
    /// this indicates that the underlying stream to decide how much data to
    /// accumulate before producing output in order to maximize compression.
    None = ffi::MZ_NO_FLUSH as isize,

    /// All pending output is flushed to the output buffer and the output is
    /// aligned on a byte boundary so that the decompressor can get all input
    /// data available so far.
    ///
    /// Flushing may degrade comperssion for some compression algorithms and so
    /// it should only be used when necessary. This will complete the current
    /// deflate block and follow it with an empty stored block.
    Sync = ffi::MZ_SYNC_FLUSH as isize,

    /// All pending output is flushed to the output buffer, but the output is
    /// not aligned to a byte boundary.
    ///
    /// All of the input data so far will be available to the decompressor (as
    /// with `Flush::Sync`. This completes the current deflate block and follows
    /// it with an empty fixed codes block that is 10 bites long, and it assures
    /// that enough bytes are output in order for the decompessor to finish the
    /// block before the empty fixed code block.
    Partial = ffi::MZ_PARTIAL_FLUSH as isize,

    /// A deflate block is completed and emitted, as for `Flush::Sync`, but the
    /// output is not aligned on a byte boundary and up to seven vits of the
    /// current block are held to be written as the next byte after the next
    /// deflate block is completed.
    ///
    /// In this case the decompressor may not be provided enough bits at this
    /// point in order to complete decompression of the data provided so far to
    /// the compressor, it may need to wait for the next block to be emitted.
    /// This is for advanced applications that need to control the emission of
    /// deflate blocks.
    Block = ffi::MZ_BLOCK as isize,

    /// All output is flushed as with `Flush::Sync` and the compression state is
    /// reset so decompression can restart from this point if previous
    /// compressed data has been damaged or if random access is desired.
    ///
    /// Using this option too often can seriously degrade compression.
    Full = ffi::MZ_FULL_FLUSH as isize,

    /// Pending input is processed and pending output is flushed.
    ///
    /// The return value may indicate that the stream is not yet done and more
    /// data has yet to be processed.
    Finish = ffi::MZ_FINISH as isize,
}

#[doc(hidden)]
pub trait Direction {
    unsafe fn destroy(stream: *mut ffi::mz_stream) -> c_int;
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
            Stream {
                raw: state,
                _marker: marker::PhantomData,
            }
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
            Stream {
                raw: state,
                _marker: marker::PhantomData,
            }
        }
    }
}

impl<T: Direction> Stream<T> {
    pub fn total_in(&self) -> u64 {
        self.raw.total_in as u64
    }

    pub fn total_out(&self) -> u64 {
        self.raw.total_out as u64
    }
}

impl Stream<Decompress> {
    pub fn decompress(&mut self,
                      input: &[u8],
                      output: &mut [u8],
                      flush: Flush)
                      -> c_int {
        self.raw.next_in = input.as_ptr();
        self.raw.avail_in = input.len() as c_uint;
        self.raw.next_out = output.as_mut_ptr();
        self.raw.avail_out = output.len() as c_uint;
        unsafe { ffi::mz_inflate(&mut self.raw, flush as c_int) }
    }

    pub fn decompress_vec(&mut self,
                          input: &[u8],
                          output: &mut Vec<u8>,
                          flush: Flush)
                          -> c_int {
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
    pub fn compress(&mut self,
                    input: &[u8],
                    output: &mut [u8],
                    flush: Flush)
                    -> c_int {
        self.raw.next_in = input.as_ptr() as *mut _;
        self.raw.avail_in = input.len() as c_uint;
        self.raw.next_out = output.as_mut_ptr();
        self.raw.avail_out = output.len() as c_uint;
        unsafe { ffi::mz_deflate(&mut self.raw, flush as c_int) }
    }

    pub fn compress_vec(&mut self,
                        input: &[u8],
                        output: &mut Vec<u8>,
                        flush: Flush)
                        -> c_int {
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

    pub fn reset(&mut self) -> c_int {
        unsafe { ffi::mz_deflateReset(&mut self.raw) }
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
