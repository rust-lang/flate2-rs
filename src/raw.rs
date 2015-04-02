//! Raw un-exported bindings to miniz for encoding/decoding

use std::io::prelude::*;
use std::io;
use libc;

use Compression;
use ffi;
use stream::{Stream, Compress, Decompress, Direction, Flush};

pub struct EncoderWriter<W: Write>(InnerWrite<W, Compress>);
pub struct DecoderWriter<W: Write>(InnerWrite<W, Decompress>);

struct InnerWrite<W, D: Direction> {
    inner: Option<W>,
    stream: Stream<D>,
    buf: Vec<u8>,
}

pub struct EncoderReader<R: Read>(InnerRead<R, Compress>);
pub struct DecoderReader<R: Read>(InnerRead<R, Decompress>);

struct InnerRead<R, D: Direction> {
    inner: R,
    stream: Stream<D>,
    buf: Vec<u8>, // TODO: this should be Box<[u8]>
    pos: usize,
    cap: usize,
}

impl<W: Write> EncoderWriter<W> {
    pub fn new(w: W, level: Compression, raw: bool, buf: Vec<u8>)
               -> EncoderWriter<W> {
        EncoderWriter(InnerWrite {
            inner: Some(w),
            stream: Stream::new_compress(level, raw),
            buf: buf,
        })
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.0.finish(&mut |stream, inner| {
            stream.compress_vec(&[], inner, Flush::Finish)
        })
    }

    pub fn into_inner(mut self) -> W { self.0.inner.take().unwrap() }

    pub fn take_inner(&mut self) -> W { self.0.inner.take().unwrap() }

    pub fn write_all_raw(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.inner.as_mut().unwrap().write_all(buf)
    }

    pub fn unwrapped(&self) -> bool {
        self.0.inner.is_none()
    }
}

impl<W: Write> Write for EncoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // miniz isn't guaranteed to actually write any of the buffer provided,
        // it may be in a flushing mode where it's just giving us data before
        // we're actually giving it any data. We don't want to spuriously return
        // `Ok(0)` when possible as it will cause calls to write_all() to fail.
        // As a result we execute this in a loop to ensure that we try our
        // darndest to write the data.
        loop {
            let n = try!(self.0.write(&mut |stream, inner| {
                stream.compress_vec(buf, inner, Flush::None)
            }));
            if buf.len() == 0 || n != 0 {
                return Ok(n)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.finish(&mut |stream, inner| {
            stream.compress_vec(&[], inner, Flush::Sync)
        }).and_then(|()| self.0.inner.as_mut().unwrap().flush())
    }
}

impl<W: Write> Drop for EncoderWriter<W> {
    fn drop(&mut self) {
        if self.0.inner.is_some() {
            let _ = self.finish();
        }
    }
}

impl<W: Write> DecoderWriter<W> {
    pub fn new(w: W, raw: bool, buf: Vec<u8>) -> DecoderWriter<W> {
        DecoderWriter(InnerWrite {
            inner: Some(w),
            stream: Stream::new_decompress(raw),
            buf: buf,
        })
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.0.finish(&mut |stream, inner| {
            stream.decompress_vec(&[], inner, Flush::Finish)
        })
    }

    pub fn into_inner(mut self) -> W { self.0.inner.take().unwrap() }
}

impl<W: Write> Write for DecoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // See EncoderWriter::write for why this is in a loop
        loop {
            let n = try!(self.0.write(&mut |stream, inner| {
                stream.decompress_vec(buf, inner, Flush::None)
            }));
            if buf.len() == 0 || n != 0 {
                return Ok(n)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.finish(&mut |stream, inner| {
            stream.decompress_vec(&[], inner, Flush::Sync)
        }).and_then(|()| self.0.inner.as_mut().unwrap().flush())
    }
}

impl<W: Write, D: Direction> InnerWrite<W, D> {
    fn write<F>(&mut self, f: &mut F) -> io::Result<usize>
        where F: FnMut(&mut Stream<D>, &mut Vec<u8>) -> libc::c_int
    {
        let writer = self.inner.as_mut().unwrap();
        if self.buf.len() > 0 {
            try!(writer.write_all(&self.buf));
            self.buf.truncate(0);
        }

        let before_in = self.stream.total_in();
        let ret = f(&mut self.stream, &mut self.buf);
        let written = (self.stream.total_in() - before_in) as usize;
        match ret {
            ffi::MZ_OK | ffi::MZ_BUF_ERROR | ffi::MZ_STREAM_END => Ok(written),
            n => panic!("unexpected return {}", n),
        }
    }

    fn finish<F>(&mut self, f: &mut F) -> io::Result<()>
        where F: FnMut(&mut Stream<D>, &mut Vec<u8>) -> libc::c_int
    {
        // Unfortunately miniz doesn't actually tell us when we're done with
        // pulling out all the data from the internal stream. To remedy this we
        // have to continually ask the stream for more memory until it doesn't
        // give us a chunk of memory the same size as our own internal buffer,
        // at which point we assume it's reached the end.
        let mut cont = true;
        while cont {
            try!(self.write(f));
            let inner = self.inner.as_mut().unwrap();
            if self.buf.len() > 0 {
                try!(inner.write_all(&self.buf));
            }
            cont = self.buf.len() == self.buf.capacity();
            self.buf.truncate(0);
        }
        Ok(())
    }
}

impl<R: Read> EncoderReader<R> {
    pub fn new(w: R, level: Compression, raw: bool, buf: Vec<u8>)
               -> EncoderReader<R> {
        EncoderReader(InnerRead {
            inner: w,
            stream: Stream::new_compress(level, raw),
            buf: buf,
            cap: 0,
            pos: 0,
        })
    }
    pub fn get_ref(&self) -> &R { &self.0.inner }
    pub fn into_inner(self) -> R { self.0.inner }
}

impl<R: Read> Read for EncoderReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(|stream, inner, flush| stream.compress(inner, buf, flush))
    }
}

impl<R: Read> DecoderReader<R> {
    pub fn new(r: R, raw: bool, buf: Vec<u8>) -> DecoderReader<R> {
        DecoderReader(InnerRead {
            inner: r,
            stream: Stream::new_decompress(raw),
            buf: buf,
            pos: 0,
            cap: 0,
        })
    }

    pub fn into_inner(self) -> R { self.0.inner }

    pub fn read_raw(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut from = &self.0.buf[self.0.pos..self.0.cap];
        match try!(Read::read(&mut from, buf)) {
            0 => {}
            n => { self.0.pos += n; return Ok(n) }
        }
        self.0.inner.read(buf)
    }
}

impl<R: Read> Read for DecoderReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(|stream, inner, flush| stream.decompress(inner, buf, flush))
    }
}

impl<R: Read, D: Direction> InnerRead<R, D> {
    fn read<F>(&mut self, mut f: F) -> io::Result<usize>
        where F: FnMut(&mut Stream<D>, &[u8], Flush) -> libc::c_int
    {
        loop {
            let mut eof = false;
            if self.pos == self.cap {
                self.cap = try!(self.inner.read(&mut self.buf));
                self.pos = 0;
                eof = self.cap == 0;
            }

            let before_out = self.stream.total_out();
            let before_in = self.stream.total_in();
            let ret = f(&mut self.stream, &self.buf[self.pos..self.cap],
                        if eof {Flush::Finish} else {Flush::None});
            let read = (self.stream.total_out() - before_out) as usize;
            self.pos += (self.stream.total_in() - before_in) as usize;

            return match ret {
                ffi::MZ_OK | ffi::MZ_BUF_ERROR => {
                    // If we haven't ready any data and we haven't hit EOF yet,
                    // then we need to keep asking for more data because if we
                    // return that 0 bytes of data have been read then it will
                    // be interpreted as EOF.
                    if read == 0 && !eof { continue }
                    Ok(read)
                }
                ffi::MZ_STREAM_END => return Ok(read),
                ffi::MZ_DATA_ERROR => {
                    Err(io::Error::new(io::ErrorKind::InvalidInput,
                                       "corrupt deflate stream"))
                }
                n => panic!("unexpected return {}", n),
            }
        }
    }
}
