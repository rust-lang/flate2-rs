//! A DEFLATE-based stream compression/decompression library
//!
//! This library is meant to supplement/replace the standard distributon's
//! libflate library by providing a streaming encoder/decoder rather than purely
//! in in-memory encoder/decoder.
//!
//! Like with libflate, flate2 is based on [`miniz.c`][1]
//!
//! [1]: https://code.google.com/p/miniz/
//!
//! # Organization
//!
//! This crate contains mainly of two modules, `reader` and `writer`. Each
//! module contains a number of types used to encode and decode various streams
//! of data. All types in the `writer` module work on instances of `Writer`,
//! whereas all types in the `reader` module work on instances of `Reader`.
//!
//! Other various types are provided at the top-level of the crate for
//! mangement and dealing with encoders/decoders.
//!
//! # Helper traits
//!
//! There are two helper traits, provided, `FlateReader` and `FlateWriter`.
//! These provide convenience methods for creating a decoder/encoder out of an
//! already existing stream to chain construction.

#![deny(missing_doc)]
#![feature(unsafe_destructor)]

extern crate libc;

use std::io::IoResult;

pub use gz::Builder as GzBuilder;
pub use gz::Header as GzHeader;

mod crc;
mod deflate;
mod ffi;
mod gz;
mod raw;
mod zlib;

/// Types which operate over `Reader` streams, both encoders and decoders for
/// various formats.
pub mod reader {
    pub use deflate::EncoderReader as DeflateEncoder;
    pub use deflate::DecoderReader as DeflateDecoder;
    pub use zlib::EncoderReader as ZlibEncoder;
    pub use zlib::DecoderReader as ZlibDecoder;
    pub use gz::EncoderReader as GzEncoder;
    pub use gz::DecoderReader as GzDecoder;
}

/// Types which operate over `Writer` streams, both encoders and decoders for
/// various formats.
pub mod writer {
    pub use deflate::EncoderWriter as DeflateEncoder;
    pub use deflate::DecoderWriter as DeflateDecoder;
    pub use zlib::EncoderWriter as ZlibEncoder;
    pub use zlib::DecoderWriter as ZlibDecoder;
    pub use gz::EncoderWriter as GzEncoder;
}

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

/// A helper trait to create encoder/decoders with method syntax.
pub trait FlateReader: Reader {
    /// Consume this reader to create a compression stream at the specified
    /// compression level.
    fn gz_encode(self, lvl: CompressionLevel) -> reader::GzEncoder<Self> {
        reader::GzEncoder::new(self, lvl)
    }

    /// Consume this reader to create a decompression stream of this stream.
    fn gz_decode(self) -> IoResult<reader::GzDecoder<Self>> {
        reader::GzDecoder::new(self)
    }

    /// Consume this reader to create a compression stream at the specified
    /// compression level.
    fn zlib_encode(self, lvl: CompressionLevel) -> reader::ZlibEncoder<Self> {
        reader::ZlibEncoder::new(self, lvl)
    }

    /// Consume this reader to create a decompression stream of this stream.
    fn zlib_decode(self) -> reader::ZlibDecoder<Self> {
        reader::ZlibDecoder::new(self)
    }

    /// Consume this reader to create a compression stream at the specified
    /// compression level.
    fn deflate_encode(self, lvl: CompressionLevel) -> reader::DeflateEncoder<Self> {
        reader::DeflateEncoder::new(self, lvl)
    }

    /// Consume this reader to create a decompression stream of this stream.
    fn deflate_decode(self) -> reader::DeflateDecoder<Self> {
        reader::DeflateDecoder::new(self)
    }
}

/// A helper trait to create encoder/decoders with method syntax.
pub trait FlateWriter: Writer {
    /// Consume this writer to create a compression stream at the specified
    /// compression level.
    fn gz_encode(self, lvl: CompressionLevel) -> writer::GzEncoder<Self> {
        writer::GzEncoder::new(self, lvl)
    }

    // TODO: coming soon to a theater near you!
    // /// Consume this writer to create a decompression stream of this stream.
    // fn gz_decode(self) -> IoResult<writer::GzDecoder<Self>> {
    //     writer::GzDecoder::new(self)
    // }

    /// Consume this writer to create a compression stream at the specified
    /// compression level.
    fn zlib_encode(self, lvl: CompressionLevel) -> writer::ZlibEncoder<Self> {
        writer::ZlibEncoder::new(self, lvl)
    }

    /// Consume this writer to create a decompression stream of this stream.
    fn zlib_decode(self) -> writer::ZlibDecoder<Self> {
        writer::ZlibDecoder::new(self)
    }

    /// Consume this writer to create a compression stream at the specified
    /// compression level.
    fn deflate_encode(self, lvl: CompressionLevel) -> writer::DeflateEncoder<Self> {
        writer::DeflateEncoder::new(self, lvl)
    }

    /// Consume this writer to create a decompression stream of this stream.
    fn deflate_decode(self) -> writer::DeflateDecoder<Self> {
        writer::DeflateDecoder::new(self)
    }
}

impl<T: Reader> FlateReader for T {}
impl<T: Writer> FlateWriter for T {}

#[cfg(test)]
mod test {
    use std::io::BufReader;
    use {FlateReader, Default};

    #[test]
    fn crazy() {
        let rdr = BufReader::new(b"foobar");
        let res = rdr.gz_encode(Default)
                        .deflate_encode(Default)
                            .zlib_encode(Default)
                            .zlib_decode()
                        .deflate_decode()
                     .gz_decode()
                     .read_to_end().unwrap();
        assert_eq!(res.as_slice(), b"foobar");
    }
}
