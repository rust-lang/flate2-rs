use std::io;
use std::io::prelude::*;

use super::bufread;
use super::{GzBuilder, GzHeader};
use crate::bufreader::BufReader;
use crate::Compression;

/// A gzip streaming encoder
///
/// This structure exposes a [`Read`] interface that will read uncompressed data
/// from the underlying reader and expose the compressed version as a [`Read`]
/// interface.
///
/// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// use flate2::Compression;
/// use flate2::read::GzEncoder;
///
/// // Return a vector containing the GZ compressed version of hello world
///
/// fn gzencode_hello_world() -> io::Result<Vec<u8>> {
///     let mut ret_vec = Vec::new();
///     let bytestring = b"hello world";
///     let mut gz = GzEncoder::new(&bytestring[..], Compression::fast());
///     let count = gz.read_to_end(&mut ret_vec)?;
///     Ok(ret_vec)
/// }
/// ```
#[derive(Debug)]
pub struct GzEncoder<R> {
    inner: bufread::GzEncoder<BufReader<R>>,
}

pub fn gz_encoder<R: Read>(inner: bufread::GzEncoder<BufReader<R>>) -> GzEncoder<R> {
    GzEncoder { inner }
}

impl<R: Read> GzEncoder<R> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// The encoder is not configured specially for the emitted header. For
    /// header configuration, see the `GzBuilder` type.
    ///
    /// The data read from the stream `r` will be compressed and available
    /// through the returned reader.
    pub fn new(r: R, level: Compression) -> GzEncoder<R> {
        GzBuilder::new().read(r, level)
    }
}

impl<R> GzEncoder<R> {
    /// Acquires a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying reader.
    ///
    /// Note that mutation of the reader may result in surprising results if
    /// this encoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    /// Returns the underlying stream, consuming this encoder
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }
}

impl<R: Read> Read for GzEncoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.inner.read(into)
    }
}

impl<R: Read + Write> Write for GzEncoder<R> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

/// A gzip streaming decoder
///
/// This structure exposes a [`Read`] interface that will consume compressed
/// data from the underlying reader and emit uncompressed data.
/// Use [`MultiGzDecoder`] if your file has multiple streams.
///
/// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// # use flate2::Compression;
/// # use flate2::write::GzEncoder;
/// use flate2::read::GzDecoder;
///
/// # fn main() {
/// #    let mut e = GzEncoder::new(Vec::new(), Compression::default());
/// #    e.write_all(b"Hello World").unwrap();
/// #    let bytes = e.finish().unwrap();
/// #    println!("{}", decode_reader(bytes).unwrap());
/// # }
/// #
/// // Uncompresses a Gz Encoded vector of bytes and returns a string or error
/// // Here &[u8] implements Read
///
/// fn decode_reader(bytes: Vec<u8>) -> io::Result<String> {
///    let mut gz = GzDecoder::new(&bytes[..]);
///    let mut s = String::new();
///    gz.read_to_string(&mut s)?;
///    Ok(s)
/// }
/// ```
#[derive(Debug)]
pub struct GzDecoder<R> {
    inner: bufread::GzDecoder<BufReader<R>>,
}

impl<R: Read> GzDecoder<R> {
    /// Creates a new decoder from the given reader, immediately parsing the
    /// gzip header.
    pub fn new(r: R) -> GzDecoder<R> {
        GzDecoder {
            inner: bufread::GzDecoder::new(BufReader::new(r)),
        }
    }
}

impl<R> GzDecoder<R> {
    /// Returns the header associated with this stream, if it was valid.
    pub fn header(&self) -> Option<&GzHeader> {
        self.inner.header()
    }

    /// Acquires a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream.
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this decoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    /// Consumes this decoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }
}

impl<R: Read> Read for GzDecoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.inner.read(into)
    }
}

impl<R: Read + Write> Write for GzDecoder<R> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

/// A gzip streaming decoder that decodes all members of a multistream
///
/// A gzip member consists of a header, compressed data and a trailer. The [gzip
/// specification](https://tools.ietf.org/html/rfc1952), however, allows multiple
/// gzip members to be joined in a single stream.  `MultiGzDecoder` will
/// decode all consecutive members while [`GzDecoder`] will only decompress the
/// first gzip member. The multistream format is commonly used in bioinformatics,
/// for example when using the BGZF compressed data. It's also useful
/// to compress large amounts of data in parallel where each thread produces one stream
/// for a chunk of input data.
///
/// This structure exposes a [`Read`] interface that will consume all gzip members
/// from the underlying reader and emit uncompressed data.
///
/// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// # use flate2::Compression;
/// # use flate2::write::GzEncoder;
/// use flate2::read::MultiGzDecoder;
///
/// # fn main() {
/// #    let mut e = GzEncoder::new(Vec::new(), Compression::default());
/// #    e.write_all(b"Hello World").unwrap();
/// #    let bytes = e.finish().unwrap();
/// #    println!("{}", decode_reader(bytes).unwrap());
/// # }
/// #
/// // Uncompresses a Gz Encoded vector of bytes and returns a string or error
/// // Here &[u8] implements Read
///
/// fn decode_reader(bytes: Vec<u8>) -> io::Result<String> {
///    let mut gz = MultiGzDecoder::new(&bytes[..]);
///    let mut s = String::new();
///    gz.read_to_string(&mut s)?;
///    Ok(s)
/// }
/// ```
#[derive(Debug)]
pub struct MultiGzDecoder<R> {
    inner: bufread::MultiGzDecoder<BufReader<R>>,
}

impl<R: Read> MultiGzDecoder<R> {
    /// Creates a new decoder from the given reader, immediately parsing the
    /// (first) gzip header. If the gzip stream contains multiple members all will
    /// be decoded.
    pub fn new(r: R) -> MultiGzDecoder<R> {
        MultiGzDecoder {
            inner: bufread::MultiGzDecoder::new(BufReader::new(r)),
        }
    }
}

impl<R> MultiGzDecoder<R> {
    /// Returns the current header associated with this stream, if it's valid.
    pub fn header(&self) -> Option<&GzHeader> {
        self.inner.header()
    }

    /// Acquires a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream.
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this decoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    /// Consumes this decoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }
}

impl<R: Read> Read for MultiGzDecoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.inner.read(into)
    }
}

impl<R: Read + Write> Write for MultiGzDecoder<R> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind, Read, Result, Write};

    use super::GzDecoder;

    //a cursor turning EOF into blocking errors
    #[derive(Debug)]
    pub struct BlockingCursor {
        pub cursor: Cursor<Vec<u8>>,
    }

    impl BlockingCursor {
        pub fn new() -> BlockingCursor {
            BlockingCursor {
                cursor: Cursor::new(Vec::new()),
            }
        }

        pub fn set_position(&mut self, pos: u64) {
            return self.cursor.set_position(pos);
        }
    }

    impl Write for BlockingCursor {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            return self.cursor.write(buf);
        }
        fn flush(&mut self) -> Result<()> {
            return self.cursor.flush();
        }
    }

    impl Read for BlockingCursor {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            //use the cursor, except it turns eof into blocking error
            let r = self.cursor.read(buf);
            match r {
                Err(ref err) => {
                    if err.kind() == ErrorKind::UnexpectedEof {
                        return Err(ErrorKind::WouldBlock.into());
                    }
                }
                Ok(0) => {
                    //regular EOF turned into blocking error
                    return Err(ErrorKind::WouldBlock.into());
                }
                Ok(_n) => {}
            }
            return r;
        }
    }

    #[test]
    fn blocked_partial_header_read() {
        // this is a reader which receives data afterwards
        let mut r = BlockingCursor::new();
        let data = vec![1, 2, 3];

        match r.write_all(&data) {
            Ok(()) => {}
            _ => {
                panic!("Unexpected result for write_all");
            }
        }
        r.set_position(0);

        // this is unused except for the buffering
        let mut decoder = GzDecoder::new(r);
        let mut out = Vec::with_capacity(7);
        match decoder.read(&mut out) {
            Err(e) => {
                assert_eq!(e.kind(), ErrorKind::WouldBlock);
            }
            _ => {
                panic!("Unexpected result for decoder.read");
            }
        }
    }
}
