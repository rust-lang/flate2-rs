//! A DEFLATE-based stream compression/decompression library
//!
//! This library is meant to supplement/replace the standard distributon's
//! libflate library by providing a streaming encoder/decoder rather than purely
//! in in-memory encoder/decoder.
//!
//! Like with libflate, flate2 is based on [`miniz.c`][1]
//!
//! [1]: https://code.google.com/p/miniz/

#![deny(missing_doc)]
#![feature(unsafe_destructor)]

extern crate libc;

use std::io;
use std::io::IoResult;
use std::mem;

pub use DeflateEncoder = deflate::Encoder;
pub use DeflateDecoder = deflate::Decoder;
pub use ZlibEncoder = zlib::Encoder;
pub use ZlibDecoder = zlib::Decoder;
pub use GzEncoder = gz::Encoder;
pub use GzDecoder = gz::Decoder;

mod ffi;
pub mod deflate;
pub mod zlib;
pub mod gz;

struct Encoder<W> {
    inner: Option<W>,
    stream: Stream,
    buf: Vec<u8>,
}

struct Decoder<R> {
    inner: R,
    stream: Stream,
    pos: uint,
    buf: Vec<u8>,
}

enum Flavor { Deflate, Inflate }

struct Stream(ffi::mz_stream, Flavor);

/// When compressing data, the compression level can be specified by a value in
/// this enum.
pub enum CompressionLevel {
    /// No compression is to be performed, this may actually inflate data
    /// slightly when encoding.
    NoCompression = 0,
    /// Optimize for the best speed of encoding.
    BestSpeed = 1,
    /// Optimize for the size of data being encoded.
    BestCompression = 9,
    /// Choose the default compression, a balance between speed and size.
    Default = 6,
}

impl<W: Writer> Encoder<W> {
    fn new(w: W, level: CompressionLevel, raw: bool,
           buf: Vec<u8>) -> Encoder<W> {
        let mut state: ffi::mz_stream = unsafe { mem::zeroed() };
        let ret = unsafe {
            ffi::mz_deflateInit2(&mut state,
                                 level as libc::c_int,
                                 ffi::MZ_DEFLATED,
                                 if raw {
                                     -ffi::MZ_DEFAULT_WINDOW_BITS
                                 } else {
                                     ffi::MZ_DEFAULT_WINDOW_BITS
                                 },
                                 9,
                                 ffi::MZ_DEFAULT_STRATEGY)
        };
        assert_eq!(ret, 0);

        Encoder {
            inner: Some(w),
            stream: Stream(state, Deflate),
            buf: buf,
        }
    }

    fn do_finish(&mut self) -> IoResult<()> {
        try!(self.deflate([], ffi::MZ_FINISH));
        self.inner.get_mut_ref().write(self.buf.as_slice())
    }

    fn deflate(&mut self, mut buf: &[u8], flush: libc::c_int) -> IoResult<()> {
        let cap = self.buf.capacity();
        loop {
            self.stream.next_in = buf.as_ptr();
            self.stream.avail_in = buf.len() as libc::c_uint;
            let cur_len = self.buf.len();
            self.stream.next_out = self.buf.mut_slice_from(cur_len).as_mut_ptr();
            self.stream.avail_out = (cap - cur_len) as libc::c_uint;

            let before_out = self.stream.total_out;
            let before_in = self.stream.total_in;

            let ret = unsafe {
                let ret = ffi::mz_deflate(&mut *self.stream, flush);
                self.buf.set_len((self.stream.total_out - before_out) as uint);
                ret
            };
            buf = buf.slice_from((self.stream.total_in - before_in) as uint);

            match ret {
                ffi::MZ_OK => {}
                ffi::MZ_STREAM_END => return Ok(()),
                ffi::MZ_BUF_ERROR => {
                    try!(self.inner.get_mut_ref().write(self.buf.as_slice()));
                    self.buf.truncate(0);
                }
                n => fail!("unexpected return {}", n),
            }
            if buf.len() == 0 { return Ok(()) }
        }
    }
}

impl<W: Writer> Writer for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        self.deflate(buf, ffi::MZ_NO_FLUSH)
    }

    fn flush(&mut self) -> IoResult<()> {
        self.deflate([], ffi::MZ_SYNC_FLUSH).and_then(|_| {
            self.inner.get_mut_ref().flush()
        })
    }
}

#[unsafe_destructor]
impl<W: Writer> Drop for Encoder<W> {
    fn drop(&mut self) {
        match self.inner {
            Some(..) => { let _ = self.do_finish(); }
            None => {}
        }
    }
}

impl<R: Reader> Decoder<R> {
    /// Same as `new`, but the intermediate buffer for data is specified.
    ///
    /// Note that the capacity of the intermediate buffer is never increased,
    /// and it is recommended for it to be large.
    fn new(r: R, raw: bool, buf: Vec<u8>) -> Decoder<R> {
        let mut state: ffi::mz_stream = unsafe { mem::zeroed() };
        let ret = unsafe {
            ffi::mz_inflateInit2(&mut state,
                                 if raw {
                                     -ffi::MZ_DEFAULT_WINDOW_BITS
                                 } else {
                                     ffi::MZ_DEFAULT_WINDOW_BITS
                                 })
        };
        assert_eq!(ret, 0);

        Decoder {
            inner: r,
            stream: Stream(state, Inflate),
            pos: 0,
            buf: buf,
        }
    }
}

impl<R: Reader> Reader for Decoder<R> {
    fn read(&mut self, into: &mut [u8]) -> IoResult<uint> {
        let cap = self.buf.capacity();
        let mut read = 0;
        while read < into.len() {
            if self.pos == self.buf.len() {
                self.buf.truncate(0);
                try!(self.inner.push(cap, &mut self.buf));
                self.pos = 0;
            }

            self.stream.next_in = self.buf.slice_from(self.pos).as_ptr();
            self.stream.avail_in = (self.buf.len() - self.pos) as libc::c_uint;
            self.stream.next_out = into.mut_slice_from(read).as_mut_ptr();
            self.stream.avail_out = (into.len() - read) as libc::c_uint;

            let before_out = self.stream.total_out;
            let before_in = self.stream.total_in;

            let ret = unsafe {
                ffi::mz_inflate(&mut *self.stream, ffi::MZ_NO_FLUSH)
            };
            read += (self.stream.total_out - before_out) as uint;
            self.pos += (self.stream.total_in - before_in) as uint;

            match ret {
                ffi::MZ_OK => {}
                ffi::MZ_STREAM_END if read > 0 => break,
                ffi::MZ_STREAM_END => {
                    return Err(io::standard_error(io::EndOfFile))
                }
                ffi::MZ_BUF_ERROR => break,
                ffi::MZ_DATA_ERROR => {
                    return Err(io::standard_error(io::InvalidInput))
                }
                n => fail!("unexpected return {}", n),
            }
        }

        Ok(read)
    }
}

impl Deref<ffi::mz_stream> for Stream {
    fn deref<'a>(&'a self) -> &'a ffi::mz_stream {
        let Stream(ref inner, _) = *self; inner
    }
}

impl DerefMut<ffi::mz_stream> for Stream {
    fn deref_mut<'a>(&'a mut self) -> &'a mut ffi::mz_stream {
        let Stream(ref mut inner, _) = *self; inner
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            match *self {
                Stream(ref mut s, Deflate) => ffi::mz_deflateEnd(s),
                Stream(ref mut s, Inflate) => ffi::mz_inflateEnd(s),
            };
        }
    }
}
