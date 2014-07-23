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

use std::io::IoResult;
use std::io;
use std::mem;

pub use GzEncoder = gz::Encoder;
pub use GzDecoder = gz::Decoder;

/// Types which operate over `Reader` streams, both encoders and decoders for
/// various formats.
pub mod reader {
    pub use DeflateEncoder = deflate::EncoderReader;
    pub use DeflateDecoder = deflate::DecoderReader;
    pub use ZlibEncoder = zlib::EncoderReader;
    pub use ZlibDecoder = zlib::DecoderReader;
}

/// Types which operate over `Writer` streams, both encoders and decoders for
/// various formats.
pub mod writer {
    pub use DeflateEncoder = deflate::EncoderWriter;
    pub use DeflateDecoder = deflate::DecoderWriter;
    pub use ZlibEncoder = zlib::EncoderWriter;
    pub use ZlibDecoder = zlib::DecoderWriter;
}

mod ffi;
mod deflate;
mod zlib;
pub mod gz;

struct EncoderWriter<W> {
    inner: Option<W>,
    stream: Stream,
    buf: Vec<u8>,
}

struct EncoderReader<R> {
    inner: R,
    stream: Stream,
    buf: Vec<u8>,
    pos: uint,
}

struct DecoderReader<R> {
    inner: R,
    stream: Stream,
    pos: uint,
    buf: Vec<u8>,
}

struct DecoderWriter<W> {
    inner: Option<W>,
    stream: Stream,
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

impl<W: Writer> EncoderWriter<W> {
    fn new(w: W, level: CompressionLevel, raw: bool,
           buf: Vec<u8>) -> EncoderWriter<W> {
        EncoderWriter {
            inner: Some(w),
            stream: Stream::new(Deflate, raw, level),
            buf: buf,
        }
    }

    fn do_finish(&mut self) -> IoResult<()> {
        try!(self.stream.write([], ffi::MZ_FINISH, &mut self.buf,
                               self.inner.get_mut_ref(), ffi::mz_deflate));
        try!(self.inner.get_mut_ref().write(self.buf.as_slice()));
        self.buf.truncate(0);
        Ok(())
    }
}

impl<W: Writer> Writer for EncoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        self.stream.write(buf, ffi::MZ_NO_FLUSH, &mut self.buf,
                          self.inner.get_mut_ref(), ffi::mz_deflate)
    }

    fn flush(&mut self) -> IoResult<()> {
        let inner = self.inner.get_mut_ref();
        try!(self.stream.write([], ffi::MZ_SYNC_FLUSH, &mut self.buf, inner,
                               ffi::mz_deflate));
        inner.flush()
    }
}

#[unsafe_destructor]
impl<W: Writer> Drop for EncoderWriter<W> {
    fn drop(&mut self) {
        match self.inner {
            Some(..) => { let _ = self.do_finish(); }
            None => {}
        }
    }
}

impl<R: Reader> EncoderReader<R> {
    fn new(w: R, level: CompressionLevel, raw: bool,
           buf: Vec<u8>) -> EncoderReader<R> {
        EncoderReader {
            inner: w,
            stream: Stream::new(Deflate, raw, level),
            buf: buf,
            pos: 0,
        }
    }
}

impl<R: Reader> Reader for EncoderReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<uint> {
        self.stream.read(buf, &mut self.buf, &mut self.pos,
                         &mut self.inner, ffi::mz_deflate)
    }
}

impl<R: Reader> DecoderReader<R> {
    /// Same as `new`, but the intermediate buffer for data is specified.
    ///
    /// Note that the capacity of the intermediate buffer is never increased,
    /// and it is recommended for it to be large.
    fn new(r: R, raw: bool, buf: Vec<u8>) -> DecoderReader<R> {
        DecoderReader {
            inner: r,
            stream: Stream::new(Inflate, raw, NoCompression),
            pos: 0,
            buf: buf,
        }
    }
}

impl<R: Reader> Reader for DecoderReader<R> {
    fn read(&mut self, into: &mut [u8]) -> IoResult<uint> {
        self.stream.read(into, &mut self.buf, &mut self.pos,
                         &mut self.inner, ffi::mz_inflate)
    }
}

impl<W: Writer> DecoderWriter<W> {
    fn new(w: W, raw: bool, buf: Vec<u8>) -> DecoderWriter<W> {
        DecoderWriter {
            inner: Some(w),
            stream: Stream::new(Inflate, raw, NoCompression),
            buf: buf,
        }
    }

    fn do_finish(&mut self) -> IoResult<()> {
        try!(self.stream.write([], ffi::MZ_FINISH, &mut self.buf,
                               self.inner.get_mut_ref(), ffi::mz_inflate));
        try!(self.inner.get_mut_ref().write(self.buf.as_slice()));
        self.buf.truncate(0);
        Ok(())
    }
}

impl<W: Writer> Writer for DecoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        self.stream.write(buf, ffi::MZ_NO_FLUSH, &mut self.buf,
                          self.inner.get_mut_ref(), ffi::mz_inflate)
    }

    fn flush(&mut self) -> IoResult<()> {
        let inner = self.inner.get_mut_ref();
        try!(self.stream.write([], ffi::MZ_SYNC_FLUSH, &mut self.buf, inner,
                               ffi::mz_inflate));
        inner.flush()
    }
}

impl Stream {
    pub fn new(kind: Flavor, raw: bool, level: CompressionLevel) -> Stream {
        let mut state: ffi::mz_stream = unsafe { mem::zeroed() };
        let ret = match kind {
            Deflate => unsafe {
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
            },
            Inflate => unsafe {
                ffi::mz_inflateInit2(&mut state,
                                     if raw {
                                         -ffi::MZ_DEFAULT_WINDOW_BITS
                                     } else {
                                         ffi::MZ_DEFAULT_WINDOW_BITS
                                     })
            }
        };
        assert_eq!(ret, 0);
        Stream(state, kind)
    }

    fn read<R: Reader>(&mut self, into: &mut [u8], buf: &mut Vec<u8>,
                       pos: &mut uint, reader: &mut R,
                       f: unsafe extern fn(*mut ffi::mz_stream,
                                           libc::c_int) -> libc::c_int)
                       -> IoResult<uint> {

        let cap = buf.capacity();
        let mut read = 0;
        let mut eof = false;
        while read < into.len() {
            if *pos == buf.len() {
                buf.truncate(0);
                match reader.push(cap, buf) {
                    Ok(..) => {}
                    Err(ref e) if e.kind == io::EndOfFile => eof = true,
                    Err(e) => return Err(e),
                }
                *pos = 0;
            }

            self.next_in = buf.slice_from(*pos).as_ptr();
            self.avail_in = (buf.len() - *pos) as libc::c_uint;
            self.next_out = into.mut_slice_from(read).as_mut_ptr();
            self.avail_out = (into.len() - read) as libc::c_uint;

            let before_out = self.total_out;
            let before_in = self.total_in;

            let flush = if eof {ffi::MZ_FINISH} else {ffi::MZ_NO_FLUSH};
            let ret = unsafe { f(&mut **self, flush) };
            read += (self.total_out - before_out) as uint;
            *pos += (self.total_in - before_in) as uint;

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

    fn write<W: Writer>(&mut self, mut buf: &[u8], flush: libc::c_int,
                        into: &mut Vec<u8>, writer: &mut W,
                        f: unsafe extern fn(*mut ffi::mz_stream,
                                            libc::c_int) -> libc::c_int)
                        -> IoResult<()> {
        let cap = into.capacity();
        while buf.len() > 0 || flush == ffi::MZ_FINISH {
            self.next_in = buf.as_ptr();
            self.avail_in = buf.len() as libc::c_uint;
            let cur_len = into.len();
            self.next_out = into.mut_slice_from(cur_len).as_mut_ptr();
            self.avail_out = (cap - cur_len) as libc::c_uint;

            let before_out = self.total_out;
            let before_in = self.total_in;

            let ret = unsafe {
                let ret = f(&mut **self, flush);
                into.set_len(cur_len + (self.total_out - before_out) as uint);
                ret
            };
            buf = buf.slice_from((self.total_in - before_in) as uint);

            if cap - cur_len == 0 || ret == ffi::MZ_BUF_ERROR {
                try!(writer.write(into.as_slice()));
                into.truncate(0);
            }
            match ret {
                ffi::MZ_OK => {},
                ffi::MZ_STREAM_END => return Ok(()),
                ffi::MZ_BUF_ERROR => {}
                n => fail!("unexpected return {}", n),
            }
        }

        Ok(())
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
