//! ZLIB compression and decompression of streams

use std::io::IoResult;

use raw;

/// A ZLIB encoder, or compressor.
///
/// This structure implements a `Writer` interface and takes a stream of
/// uncompressed data, writing the compressed data to the wrapped writer.
pub struct EncoderWriter<W> {
    inner: raw::EncoderWriter<W>,
}

/// A ZLIB encoder, or compressor.
///
/// This structure implements a `Reader` interface and will read uncompressed
/// data from an underlying stream and emit a stream of compressed data.
pub struct EncoderReader<R> {
    inner: raw::EncoderReader<R>,
}

/// A ZLIB decoder, or decompressor.
///
/// This structure implements a `Reader` interface and takes a stream of
/// compressed data as input, providing the decompressed data when read from.
pub struct DecoderReader<R> {
    inner: raw::DecoderReader<R>,
}

/// A ZLIB decoder, or decompressor.
///
/// This structure implements a `Writer` and will emit a stream of decompressed
/// data when fed a stream of compressed data.
pub struct DecoderWriter<W> {
    inner: raw::DecoderWriter<W>,
}

impl<W: Writer> EncoderWriter<W> {
    /// Creates a new encoder which will write compressed data to the stream
    /// given at the given compression level.
    ///
    /// When this encoder is dropped or unwrapped the final pieces of data will
    /// be flushed.
    pub fn new(w: W, level: ::CompressionLevel) -> EncoderWriter<W> {
        EncoderWriter {
            inner: raw::EncoderWriter::new(w, level, false,
                                           Vec::with_capacity(128 * 1024))
        }
    }

    /// Consumes this encoder, flushing the output stream.
    ///
    /// This will flush the underlying data stream and then return the contained
    /// writer if the flush succeeded.
    pub fn finish(mut self) -> IoResult<W> {
        try!(self.inner.do_finish());
        Ok(self.inner.inner.take().unwrap())
    }
}

impl<W: Writer> Writer for EncoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> { self.inner.write(buf) }
    fn flush(&mut self) -> IoResult<()> { self.inner.flush() }
}

impl<R: Reader> EncoderReader<R> {
    /// Creates a new encoder which will read uncompressed data from the given
    /// stream and emit the compressed stream.
    pub fn new(r: R, level: ::CompressionLevel) -> EncoderReader<R> {
        EncoderReader {
            inner: raw::EncoderReader::new(r, level, false,
                                           Vec::with_capacity(128 * 1024))
        }
    }

    /// Consumes this encoder, returning the underlying reader.
    pub fn unwrap(self) -> R {
        self.inner.inner
    }
}

impl<R: Reader> Reader for EncoderReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<uint> { self.inner.read(buf) }
}

impl<R: Reader> DecoderReader<R> {
    /// Creates a new decoder which will decompress data read from the given
    /// stream.
    pub fn new(r: R) -> DecoderReader<R> {
        DecoderReader::new_with_buf(r, Vec::with_capacity(128 * 1024))
    }

    /// Same as `new`, but the intermediate buffer for data is specified.
    ///
    /// Note that the capacity of the intermediate buffer is never increased,
    /// and it is recommended for it to be large.
    pub fn new_with_buf(r: R, buf: Vec<u8>) -> DecoderReader<R> {
        DecoderReader { inner: raw::DecoderReader::new(r, false, buf) }
    }
}

impl<R: Reader> Reader for DecoderReader<R> {
    fn read(&mut self, into: &mut [u8]) -> IoResult<uint> {
        self.inner.read(into)
    }
}

impl<W: Writer> DecoderWriter<W> {
    /// Creates a new decoder which will write uncompressed data to the stream.
    ///
    /// When this encoder is dropped or unwrapped the final pieces of data will
    /// be flushed.
    pub fn new(w: W) -> DecoderWriter<W> {
        DecoderWriter {
            inner: raw::DecoderWriter::new(w, false, Vec::with_capacity(128 * 1024))
        }
    }

    /// Consumes this encoder, flushing the output stream.
    ///
    /// This will flush the underlying data stream and then return the contained
    /// writer if the flush succeeded.
    pub fn finish(mut self) -> IoResult<W> {
        try!(self.inner.do_finish());
        Ok(self.inner.inner.take().unwrap())
    }
}

impl<W: Writer> Writer for DecoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> { self.inner.write(buf) }
    fn flush(&mut self) -> IoResult<()> { self.inner.flush() }
}

#[cfg(test)]
mod tests {
    use super::{EncoderWriter, EncoderReader, DecoderReader, DecoderWriter};
    use std::io::{MemWriter, MemReader, BufReader};
    use std::rand::{task_rng, Rng};

    use {Default};

    #[test]
    fn roundtrip() {
        let mut real = Vec::new();
        let mut w = EncoderWriter::new(MemWriter::new(), Default);
        let v = task_rng().gen_iter::<u8>().take(1024).collect::<Vec<_>>();
        for _ in range(0u, 200) {
            let to_write = v.slice_to(task_rng().gen_range(0, v.len()));
            real.push_all(to_write);
            w.write(to_write).unwrap();
        }
        let result = w.finish().unwrap();
        let mut r = DecoderReader::new(MemReader::new(result.unwrap()));
        assert!(r.read_to_end().unwrap() == real);
    }

    #[test]
    fn roundtrip2() {
        let v = task_rng().gen_iter::<u8>().take(1024 * 1024).collect::<Vec<_>>();
        let v = v.as_slice();
        let buf = BufReader::new(v);
        let mut r = DecoderReader::new(EncoderReader::new(buf, Default));
        assert!(r.read_to_end().unwrap().as_slice() == v);
    }

    #[test]
    fn roundtrip3() {
        let v = task_rng().gen_iter::<u8>().take(1024 * 1024).collect::<Vec<_>>();
        let v = v.as_slice();
        let mut w = EncoderWriter::new(DecoderWriter::new(MemWriter::new()),
                                       Default);
        w.write(v.as_slice()).unwrap();
        let w = w.finish().unwrap().finish().unwrap().unwrap();
        assert!(w.as_slice() == v);
    }
}


