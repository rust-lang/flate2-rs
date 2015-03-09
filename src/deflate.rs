//! DEFLATE compression and decompression of streams

use std::io::prelude::*;
use std::io;
use std::iter::repeat;

use raw;

/// A DEFLATE encoder, or compressor.
///
/// This structure implements a `Write` interface and takes a stream of
/// uncompressed data, writing the compressed data to the wrapped writer.
pub struct EncoderWriter<W: Write> {
    inner: raw::EncoderWriter<W>,
}

/// A DEFLATE encoder, or compressor.
///
/// This structure implements a `Read` interface and will read uncompressed
/// data from an underlying stream and emit a stream of compressed data.
pub struct EncoderReader<R: Read> {
    inner: raw::EncoderReader<R>,
}

/// A DEFLATE decoder, or decompressor.
///
/// This structure implements a `Read` interface and takes a stream of
/// compressed data as input, providing the decompressed data when read from.
pub struct DecoderReader<R: Read> {
    inner: raw::DecoderReader<R>,
}

/// A DEFLATE decoder, or decompressor.
///
/// This structure implements a `Write` and will emit a stream of decompressed
/// data when fed a stream of compressed data.
pub struct DecoderWriter<W: Write> {
    inner: raw::DecoderWriter<W>,
}

impl<W: Write> EncoderWriter<W> {
    /// Creates a new encoder which will write compressed data to the stream
    /// given at the given compression level.
    ///
    /// When this encoder is dropped or unwrapped the final pieces of data will
    /// be flushed.
    pub fn new(w: W, level: ::Compression) -> EncoderWriter<W> {
        EncoderWriter {
            inner: raw::EncoderWriter::new(w, level, true,
                                           Vec::with_capacity(32 * 1024)),
        }
    }

    /// Consumes this encoder, flushing the output stream.
    ///
    /// This will flush the underlying data stream and then return the contained
    /// writer if the flush succeeded.
    pub fn finish(mut self) -> io::Result<W> {
        try!(self.inner.finish());
        Ok(self.inner.into_inner())
    }
}

impl<W: Write> Write for EncoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> { self.inner.write(buf) }
    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
}

impl<R: Read> EncoderReader<R> {
    /// Creates a new encoder which will read uncompressed data from the given
    /// stream and emit the compressed stream.
    pub fn new(r: R, level: ::Compression) -> EncoderReader<R> {
        EncoderReader {
            inner: raw::EncoderReader::new(r, level, true,
                                           repeat(0).take(32 * 1024).collect()),
        }
    }

    /// Consumes this encoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.inner.into_inner()
    }
}

impl<R: Read> Read for EncoderReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: Read> DecoderReader<R> {
    /// Creates a new decoder which will decompress data read from the given
    /// stream.
    pub fn new(r: R) -> DecoderReader<R> {
        DecoderReader::new_with_buf(r, repeat(0).take(32 * 1024).collect())
    }

    /// Same as `new`, but the intermediate buffer for data is specified.
    ///
    /// Note that the capacity of the intermediate buffer is never increased,
    /// and it is recommended for it to be large.
    pub fn new_with_buf(r: R, buf: Vec<u8>) -> DecoderReader<R> {
        DecoderReader { inner: raw::DecoderReader::new(r, true, buf) }
    }

    /// Consumes this decoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.inner.into_inner()
    }
}

impl<R: Read> Read for DecoderReader<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.inner.read(into)
    }
}

impl<W: Write> DecoderWriter<W> {
    /// Creates a new decoder which will write uncompressed data to the stream.
    ///
    /// When this encoder is dropped or unwrapped the final pieces of data will
    /// be flushed.
    pub fn new(w: W) -> DecoderWriter<W> {
        DecoderWriter {
            inner: raw::DecoderWriter::new(w, true,
                                           Vec::with_capacity(32 * 1024)),
        }
    }

    /// Consumes this encoder, flushing the output stream.
    ///
    /// This will flush the underlying data stream and then return the contained
    /// writer if the flush succeeded.
    pub fn finish(mut self) -> io::Result<W> {
        try!(self.inner.finish());
        Ok(self.inner.into_inner())
    }
}

impl<W: Write> Write for DecoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> { self.inner.write(buf) }
    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
}

#[cfg(test)]
mod tests {
    use std::io::prelude::*;

    use rand::{thread_rng, Rng};

    use deflate::{EncoderWriter, EncoderReader, DecoderReader, DecoderWriter};
    use Compression::Default;

    #[test]
    fn roundtrip() {
        let mut real = Vec::new();
        let mut w = EncoderWriter::new(Vec::new(), Default);
        let v = thread_rng().gen_iter::<u8>().take(1024).collect::<Vec<_>>();
        for _ in 0..200 {
            let to_write = &v[..thread_rng().gen_range(0, v.len())];
            real.extend(to_write.iter().map(|x| *x));
            w.write_all(to_write).unwrap();
        }
        let result = w.finish().unwrap();
        let mut r = DecoderReader::new(&result[..]);
        let mut ret = Vec::new();
        r.read_to_end(&mut ret).unwrap();
        assert!(ret == real);
    }

    #[test]
    fn roundtrip2() {
        let v = thread_rng().gen_iter::<u8>().take(1024 * 1024).collect::<Vec<_>>();
        let mut r = DecoderReader::new(EncoderReader::new(&v[..], Default));
        let mut ret = Vec::new();
        r.read_to_end(&mut ret).unwrap();
        assert_eq!(ret, v);
    }

    #[test]
    fn roundtrip3() {
        let v = thread_rng().gen_iter::<u8>().take(1024 * 1024).collect::<Vec<_>>();
        let mut w = EncoderWriter::new(DecoderWriter::new(Vec::new()), Default);
        w.write_all(&v).unwrap();
        let w = w.finish().unwrap().finish().unwrap();
        assert!(w == v);
    }
}


