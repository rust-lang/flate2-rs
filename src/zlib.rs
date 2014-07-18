//! ZLIB compression and decompression of streams

use std::io::IoResult;

/// A ZLIB encoder, or compressor.
///
/// This structure implements a `Writer` interface and takes a stream of
/// uncompressed data, writing the compressed data to the wrapped writer.
pub struct Encoder<W> {
    inner: ::Encoder<W>,
}

/// A ZLIB decoder, or decompressor.
///
/// This structure implements a `Reader` interface and takes a stream of
/// compressed data as input, providing the decompressed data when read from.
pub struct Decoder<R> {
    inner: ::Decoder<R>,
}

impl<W: Writer> Encoder<W> {
    /// Creates a new encoder which will write compressed data to the stream
    /// given at the given compression level.
    ///
    /// When this encoder is dropped or unwrapped the final pieces of data will
    /// be flushed.
    pub fn new(w: W, level: ::CompressionLevel) -> Encoder<W> {
        Encoder {
            inner: ::Encoder::new(w, level, false, Vec::with_capacity(128 * 1024))
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

impl<W: Writer> Writer for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> { self.inner.write(buf) }
    fn flush(&mut self) -> IoResult<()> { self.inner.flush() }
}

impl<R: Reader> Decoder<R> {
    /// Creates a new decoder which will decompress data read from the given
    /// stream.
    pub fn new(r: R) -> Decoder<R> {
        Decoder::new_with_buf(r, Vec::with_capacity(128 * 1024))
    }

    /// Same as `new`, but the intermediate buffer for data is specified.
    ///
    /// Note that the capacity of the intermediate buffer is never increased,
    /// and it is recommended for it to be large.
    pub fn new_with_buf(r: R, buf: Vec<u8>) -> Decoder<R> {
        Decoder { inner: ::Decoder::new(r, false, buf) }
    }
}

impl<R: Reader> Reader for Decoder<R> {
    fn read(&mut self, into: &mut [u8]) -> IoResult<uint> {
        self.inner.read(into)
    }
}

#[cfg(test)]
mod tests {
    use super::{Encoder, Decoder};
    use std::io::{MemWriter, MemReader};
    use std::rand::{task_rng, Rng};

    use {Default};

    #[test]
    fn roundtrip() {
        let mut real = Vec::new();
        let mut w = Encoder::new(MemWriter::new(), Default);
        let v = task_rng().gen_iter::<u8>().take(1024).collect::<Vec<_>>();
        for _ in range(0u, 10 * 1024) {
            let to_write = v.slice_to(task_rng().gen_range(0, v.len()));
            real.push_all(to_write);
            w.write(to_write).unwrap();
        }
        let result = w.finish().unwrap();
        let mut r = Decoder::new(MemReader::new(result.unwrap()));
        assert!(r.read_to_end().unwrap() == real);
    }
}


