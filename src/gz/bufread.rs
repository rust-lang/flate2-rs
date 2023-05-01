use std::cmp;
use std::io;
use std::io::prelude::*;
use std::mem;

use super::corrupt;
use super::read_gz_header_part;
use super::Buffer;
use super::GzHeaderPartial;
use super::{GzBuilder, GzHeader};
use crate::crc::CrcReader;
use crate::deflate;
use crate::Compression;

fn copy(into: &mut [u8], from: &[u8], pos: &mut usize) -> usize {
    let min = cmp::min(into.len(), from.len() - *pos);
    for (slot, val) in into.iter_mut().zip(from[*pos..*pos + min].iter()) {
        *slot = *val;
    }
    *pos += min;
    min
}

/// A gzip streaming encoder
///
/// This structure exposes a [`BufRead`] interface that will read uncompressed data
/// from the underlying reader and expose the compressed version as a [`BufRead`]
/// interface.
///
/// [`BufRead`]: https://doc.rust-lang.org/std/io/trait.BufRead.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// use flate2::Compression;
/// use flate2::bufread::GzEncoder;
/// use std::fs::File;
/// use std::io::BufReader;
///
/// // Opens sample file, compresses the contents and returns a Vector or error
/// // File wrapped in a BufReader implements BufRead
///
/// fn open_hello_world() -> io::Result<Vec<u8>> {
///     let f = File::open("examples/hello_world.txt")?;
///     let b = BufReader::new(f);
///     let mut gz = GzEncoder::new(b, Compression::fast());
///     let mut buffer = Vec::new();
///     gz.read_to_end(&mut buffer)?;
///     Ok(buffer)
/// }
/// ```
#[derive(Debug)]
pub struct GzEncoder<R> {
    inner: deflate::bufread::DeflateEncoder<CrcReader<R>>,
    header: Vec<u8>,
    pos: usize,
    eof: bool,
}

pub fn gz_encoder<R: BufRead>(header: Vec<u8>, r: R, lvl: Compression) -> GzEncoder<R> {
    let crc = CrcReader::new(r);
    GzEncoder {
        inner: deflate::bufread::DeflateEncoder::new(crc, lvl),
        header,
        pos: 0,
        eof: false,
    }
}

impl<R: BufRead> GzEncoder<R> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// The encoder is not configured specially for the emitted header. For
    /// header configuration, see the `GzBuilder` type.
    ///
    /// The data read from the stream `r` will be compressed and available
    /// through the returned reader.
    pub fn new(r: R, level: Compression) -> GzEncoder<R> {
        GzBuilder::new().buf_read(r, level)
    }

    fn read_footer(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if self.pos == 8 {
            return Ok(0);
        }
        let crc = self.inner.get_ref().crc();
        let ref arr = [
            (crc.sum() >> 0) as u8,
            (crc.sum() >> 8) as u8,
            (crc.sum() >> 16) as u8,
            (crc.sum() >> 24) as u8,
            (crc.amount() >> 0) as u8,
            (crc.amount() >> 8) as u8,
            (crc.amount() >> 16) as u8,
            (crc.amount() >> 24) as u8,
        ];
        Ok(copy(into, arr, &mut self.pos))
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

#[inline]
fn finish(buf: &[u8; 8]) -> (u32, u32) {
    let crc = ((buf[0] as u32) << 0)
        | ((buf[1] as u32) << 8)
        | ((buf[2] as u32) << 16)
        | ((buf[3] as u32) << 24);
    let amt = ((buf[4] as u32) << 0)
        | ((buf[5] as u32) << 8)
        | ((buf[6] as u32) << 16)
        | ((buf[7] as u32) << 24);
    (crc, amt)
}

impl<R: BufRead> Read for GzEncoder<R> {
    fn read(&mut self, mut into: &mut [u8]) -> io::Result<usize> {
        let mut amt = 0;
        if self.eof {
            return self.read_footer(into);
        } else if self.pos < self.header.len() {
            amt += copy(into, &self.header, &mut self.pos);
            if amt == into.len() {
                return Ok(amt);
            }
            let tmp = into;
            into = &mut tmp[amt..];
        }
        match self.inner.read(into)? {
            0 => {
                self.eof = true;
                self.pos = 0;
                self.read_footer(into)
            }
            n => Ok(amt + n),
        }
    }
}

impl<R: BufRead + Write> Write for GzEncoder<R> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.get_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.get_mut().flush()
    }
}

/// A gzip streaming decoder
///
/// This structure consumes a [`BufRead`] interface, reading compressed data
/// from the underlying reader, and emitting uncompressed data.
///
/// [`BufRead`]: https://doc.rust-lang.org/std/io/trait.BufRead.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// # use flate2::Compression;
/// # use flate2::write::GzEncoder;
/// use flate2::bufread::GzDecoder;
///
/// # fn main() {
/// #   let mut e = GzEncoder::new(Vec::new(), Compression::default());
/// #   e.write_all(b"Hello World").unwrap();
/// #   let bytes = e.finish().unwrap();
/// #   println!("{}", decode_reader(bytes).unwrap());
/// # }
/// #
/// // Uncompresses a Gz Encoded vector of bytes and returns a string or error
/// // Here &[u8] implements BufRead
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
    inner: GzState,
    header: Option<GzHeader>,
    reader: CrcReader<deflate::bufread::DeflateDecoder<R>>,
    multi: bool,
}

#[derive(Debug)]
enum GzState {
    Header(GzHeaderPartial),
    Body,
    Finished(usize, [u8; 8]),
    Err(io::Error),
    End,
}

impl<R: BufRead> GzDecoder<R> {
    /// Creates a new decoder from the given reader, immediately parsing the
    /// gzip header.
    pub fn new(mut r: R) -> GzDecoder<R> {
        let mut part = GzHeaderPartial::new();
        let mut header = None;

        let result = {
            let mut reader = Buffer::new(&mut part, &mut r);
            read_gz_header_part(&mut reader)
        };

        let state = match result {
            Ok(()) => {
                header = Some(part.take_header());
                GzState::Body
            }
            Err(ref err) if io::ErrorKind::WouldBlock == err.kind() => GzState::Header(part),
            Err(err) => GzState::Err(err),
        };

        GzDecoder {
            inner: state,
            reader: CrcReader::new(deflate::bufread::DeflateDecoder::new(r)),
            multi: false,
            header,
        }
    }

    fn multi(mut self, flag: bool) -> GzDecoder<R> {
        self.multi = flag;
        self
    }
}

impl<R> GzDecoder<R> {
    /// Returns the header associated with this stream, if it was valid
    pub fn header(&self) -> Option<&GzHeader> {
        self.header.as_ref()
    }

    /// Acquires a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        self.reader.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream.
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this decoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.reader.get_mut().get_mut()
    }

    /// Consumes this decoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.reader.into_inner().into_inner()
    }
}

impl<R: BufRead> Read for GzDecoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let GzDecoder {
            inner,
            header,
            reader,
            multi,
        } = self;

        loop {
            *inner = match mem::replace(inner, GzState::End) {
                GzState::Header(mut part) => {
                    let result = {
                        let mut reader = Buffer::new(&mut part, reader.get_mut().get_mut());
                        read_gz_header_part(&mut reader)
                    };
                    match result {
                        Ok(()) => {
                            *header = Some(part.take_header());
                            GzState::Body
                        }
                        Err(err) if io::ErrorKind::WouldBlock == err.kind() => {
                            *inner = GzState::Header(part);
                            return Err(err);
                        }
                        Err(err) => return Err(err),
                    }
                }
                GzState::Body => {
                    if into.is_empty() {
                        *inner = GzState::Body;
                        return Ok(0);
                    }

                    let n = reader.read(into).map_err(|err| {
                        if io::ErrorKind::WouldBlock == err.kind() {
                            *inner = GzState::Body;
                        }

                        err
                    })?;

                    match n {
                        0 => GzState::Finished(0, [0; 8]),
                        n => {
                            *inner = GzState::Body;
                            return Ok(n);
                        }
                    }
                }
                GzState::Finished(pos, mut buf) => {
                    if pos < buf.len() {
                        let n = reader
                            .get_mut()
                            .get_mut()
                            .read(&mut buf[pos..])
                            .and_then(|n| {
                                if n == 0 {
                                    Err(io::ErrorKind::UnexpectedEof.into())
                                } else {
                                    Ok(n)
                                }
                            })
                            .map_err(|err| {
                                if io::ErrorKind::WouldBlock == err.kind() {
                                    *inner = GzState::Finished(pos, buf);
                                }

                                err
                            })?;

                        GzState::Finished(pos + n, buf)
                    } else {
                        let (crc, amt) = finish(&buf);

                        if crc != reader.crc().sum() || amt != reader.crc().amount() {
                            return Err(corrupt());
                        } else if *multi {
                            let is_eof = reader
                                .get_mut()
                                .get_mut()
                                .fill_buf()
                                .map(|buf| buf.is_empty())
                                .map_err(|err| {
                                    if io::ErrorKind::WouldBlock == err.kind() {
                                        *inner = GzState::Finished(pos, buf);
                                    }

                                    err
                                })?;

                            if is_eof {
                                GzState::End
                            } else {
                                reader.reset();
                                reader.get_mut().reset_data();
                                header.take();
                                GzState::Header(GzHeaderPartial::new())
                            }
                        } else {
                            GzState::End
                        }
                    }
                }
                GzState::Err(err) => return Err(err),
                GzState::End => return Ok(0),
            };
        }
    }
}

impl<R: BufRead + Write> Write for GzDecoder<R> {
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
/// gzip members to be joined in a single stream. `MultiGzDecoder` will
/// decode all consecutive members while `GzDecoder` will only decompress
/// the first gzip member. The multistream format is commonly used in
/// bioinformatics, for example when using the BGZF compressed data.
///
/// This structure exposes a [`BufRead`] interface that will consume all gzip members
/// from the underlying reader and emit uncompressed data.
///
/// [`BufRead`]: https://doc.rust-lang.org/std/io/trait.BufRead.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// # use flate2::Compression;
/// # use flate2::write::GzEncoder;
/// use flate2::bufread::MultiGzDecoder;
///
/// # fn main() {
/// #   let mut e = GzEncoder::new(Vec::new(), Compression::default());
/// #   e.write_all(b"Hello World").unwrap();
/// #   let bytes = e.finish().unwrap();
/// #   println!("{}", decode_reader(bytes).unwrap());
/// # }
/// #
/// // Uncompresses a Gz Encoded vector of bytes and returns a string or error
/// // Here &[u8] implements BufRead
///
/// fn decode_reader(bytes: Vec<u8>) -> io::Result<String> {
///    let mut gz = MultiGzDecoder::new(&bytes[..]);
///    let mut s = String::new();
///    gz.read_to_string(&mut s)?;
///    Ok(s)
/// }
/// ```
#[derive(Debug)]
pub struct MultiGzDecoder<R>(GzDecoder<R>);

impl<R: BufRead> MultiGzDecoder<R> {
    /// Creates a new decoder from the given reader, immediately parsing the
    /// (first) gzip header. If the gzip stream contains multiple members all will
    /// be decoded.
    pub fn new(r: R) -> MultiGzDecoder<R> {
        MultiGzDecoder(GzDecoder::new(r).multi(true))
    }
}

impl<R> MultiGzDecoder<R> {
    /// Returns the current header associated with this stream, if it's valid
    pub fn header(&self) -> Option<&GzHeader> {
        self.0.header()
    }

    /// Acquires a reference to the underlying reader.
    pub fn get_ref(&self) -> &R {
        self.0.get_ref()
    }

    /// Acquires a mutable reference to the underlying stream.
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this decoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.0.get_mut()
    }

    /// Consumes this decoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.0.into_inner()
    }
}

impl<R: BufRead> Read for MultiGzDecoder<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        self.0.read(into)
    }
}

#[cfg(test)]
pub mod tests {
    use crate::gz::bufread::*;
    use std::io;
    use std::io::{Cursor, Read, Write};

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
            self.cursor.set_position(pos)
        }

        pub fn position(&mut self) -> u64 {
            self.cursor.position()
        }
    }

    impl Write for BlockingCursor {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.cursor.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.cursor.flush()
        }
    }

    impl Read for BlockingCursor {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            //use the cursor, except it turns eof into blocking error
            let r = self.cursor.read(buf);
            match r {
                Err(ref err) => {
                    if err.kind() == io::ErrorKind::UnexpectedEof {
                        return Err(io::ErrorKind::WouldBlock.into());
                    }
                }
                Ok(0) => {
                    //regular EOF turned into blocking error
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                Ok(_n) => {}
            }
            r
        }
    }
    #[test]
    // test function read_and_forget of Buffer
    fn buffer_read_and_forget() {
        // this is unused except for the buffering
        let mut part = GzHeaderPartial::new();
        // this is a reader which receives data afterwards
        let mut r = BlockingCursor::new();
        let data = vec![1, 2, 3];
        let mut out = Vec::with_capacity(7);

        match r.write_all(&data) {
            Ok(()) => {}
            _ => {
                panic!("Unexpected result for write_all");
            }
        }
        r.set_position(0);

        // First read : successful for one byte
        let mut reader = Buffer::new(&mut part, &mut r);
        out.resize(1, 0);
        match reader.read_and_forget(&mut out) {
            Ok(1) => {}
            _ => {
                panic!("Unexpected result for read_and_forget with data");
            }
        }

        // Second read : incomplete for 7 bytes (we have only 2)
        out.resize(7, 0);
        match reader.read_and_forget(&mut out) {
            Err(ref err) => {
                assert_eq!(io::ErrorKind::WouldBlock, err.kind());
            }
            _ => {
                panic!("Unexpected result for read_and_forget with incomplete");
            }
        }

        // 3 more data bytes have arrived
        let pos = r.position();
        let data2 = vec![4, 5, 6];
        match r.write_all(&data2) {
            Ok(()) => {}
            _ => {
                panic!("Unexpected result for write_all");
            }
        }
        r.set_position(pos);

        // Third read : still incomplete for 7 bytes (we have 5)
        let mut reader2 = Buffer::new(&mut part, &mut r);
        match reader2.read_and_forget(&mut out) {
            Err(ref err) => {
                assert_eq!(io::ErrorKind::WouldBlock, err.kind());
            }
            _ => {
                panic!("Unexpected result for read_and_forget with more incomplete");
            }
        }

        // 3 more data bytes have arrived again
        let pos2 = r.position();
        let data3 = vec![7, 8, 9];
        match r.write_all(&data3) {
            Ok(()) => {}
            _ => {
                panic!("Unexpected result for write_all");
            }
        }
        r.set_position(pos2);

        // Fourth read : now successful for 7 bytes
        let mut reader3 = Buffer::new(&mut part, &mut r);
        match reader3.read_and_forget(&mut out) {
            Ok(7) => {
                assert_eq!(out[0], 2);
                assert_eq!(out[6], 8);
            }
            _ => {
                panic!("Unexpected result for read_and_forget with data");
            }
        }

        // Fifth read : successful for one more byte
        out.resize(1, 0);
        match reader3.read_and_forget(&mut out) {
            Ok(1) => {
                assert_eq!(out[0], 9);
            }
            _ => {
                panic!("Unexpected result for read_and_forget with data");
            }
        }
    }
}
