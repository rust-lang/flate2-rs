//! gzip compression/decompression
//!
//! [1]: http://www.gzip.org/zlib/rfc-gzip.html

use std::c_str::CString;
use std::cmp;
use std::io::{BytesReader,IoResult, IoError};
use std::io;
use std::os;
use std::slice::bytes;
use libc;

use {BestCompression, CompressionLevel, BestSpeed};
use crc::{CrcReader, Crc};
use raw;

static FHCRC: u8 = 1 << 1;
static FEXTRA: u8 = 1 << 2;
static FNAME: u8 = 1 << 3;
static FCOMMENT: u8 = 1 << 4;

/// A gzip streaming encoder
///
/// This structure exposes a `Writer` interface that will emit compressed data
/// to the underlying writer `W`.
pub struct EncoderWriter<W> {
    inner: raw::EncoderWriter<W>,
    crc: Crc,
    header: Vec<u8>,
}

/// A gzip streaming encoder
///
/// This structure exposes a `Reader` interface that will read uncompressed data
/// from the underlying reader and expose the compressed version as a `Reader`
/// interface.
pub struct EncoderReader<R> {
    inner: raw::EncoderReader<CrcReader<R>>,
    header: Vec<u8>,
    pos: uint,
    eof: bool,
}

/// A builder structure to create a new gzip Encoder.
///
/// This structure controls header configuration options such as the filename.
pub struct Builder {
    extra: Option<Vec<u8>>,
    filename: Option<CString>,
    comment: Option<CString>,
    mtime: u32,
}

/// A gzip streaming decoder
///
/// This structure exposes a `Reader` interface that will consume compressed
/// data from the underlying reader and emit uncompressed data.
pub struct DecoderReader<R> {
    inner: CrcReader<raw::DecoderReader<R>>,
    header: Header,
}

/// A structure representing the header of a gzip stream.
///
/// The header can contain metadata about the file that was compressed, if
/// present.
pub struct Header {
    extra: Option<Vec<u8>>,
    filename: Option<Vec<u8>>,
    comment: Option<Vec<u8>>,
    mtime: u32,
}

impl Builder {
    /// Create a new blank builder with no header by default.
    pub fn new() -> Builder {
        Builder {
            extra: None,
            filename: None,
            comment: None,
            mtime: 0,
        }
    }

    /// Configure the `mtime` field in the gzip header.
    pub fn mtime(mut self, mtime: u32) -> Builder {
        self.mtime = mtime;
        self
    }

    /// Configure the `extra` field in the gzip header.
    pub fn extra(mut self, extra: Vec<u8>) -> Builder {
        self.extra = Some(extra);
        self
    }

    /// Configure the `filename` field in the gzip header.
    pub fn filename<T: ToCStr>(mut self, filename: T) -> Builder {
        self.filename = Some(filename.to_c_str());
        self
    }

    /// Configure the `comment` field in the gzip header.
    pub fn comment<T: ToCStr>(mut self, comment: T) -> Builder {
        self.comment = Some(comment.to_c_str());
        self
    }

    /// Consume this builder, creating a writer encoder in the process.
    ///
    /// The data written to the returned encoder will be compressed and then
    /// written out to the supplied parameter `w`.
    pub fn writer<W: Writer>(self, w: W,
                             lvl: CompressionLevel) -> EncoderWriter<W> {
        EncoderWriter {
            inner: raw::EncoderWriter::new(w, lvl, true,
                                           Vec::with_capacity(128 * 1024)),
            crc: Crc::new(),
            header: self.into_header(lvl),
        }
    }

    /// Consume this builder, creating a reader encoder in the process.
    ///
    /// Data read from the returned encoder will be the compressed version of
    /// the data read from the given reader.
    pub fn reader<R: Reader>(self, r: R,
                             lvl: CompressionLevel) -> EncoderReader<R> {
        let crc = CrcReader::new(r);
        EncoderReader {
            inner: raw::EncoderReader::new(crc, lvl, true,
                                           Vec::with_capacity(128 * 1024)),
            header: self.into_header(lvl),
            pos: 0,
            eof: false,
        }
    }

    fn into_header(self, lvl: CompressionLevel) -> Vec<u8> {
        let Builder { extra, filename, comment, mtime } = self;
        let mut flg = 0;
        let mut header = Vec::from_elem(10, 0u8);
        match extra {
            Some(v) => {
                flg |= FEXTRA;
                header.push((v.len() >> 0) as u8);
                header.push((v.len() >> 8) as u8);
                header.push_all(v.as_slice());
            }
            None => {}
        }
        match filename {
            Some(filename) => {
                flg |= FNAME;
                header.push_all(filename.as_bytes());
            }
            None => {}
        }
        match comment {
            Some(comment) => {
                flg |= FCOMMENT;
                header.push_all(comment.as_bytes());
            }
            None => {}
        }
        header[0] = 0x1f;
        header[1] = 0x8b;
        header[2] = 8;
        header[3] = flg;
        header[4] = (mtime >>  0) as u8;
        header[5] = (mtime >>  8) as u8;
        header[6] = (mtime >> 16) as u8;
        header[7] = (mtime >> 24) as u8;
        header[8] = match lvl {
            BestCompression => 2,
            BestSpeed => 4,
            _ => 0,
        };
        header[9] = match os::consts::SYSNAME {
            "linux" => 3,
            "macos" => 7,
            "win32" => 0,
            _ => 255,
        };
        return header;
    }
}

impl<W: Writer> EncoderWriter<W> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// The encoder is not configured specially for the emitted header. For
    /// header configuration, see the `Builder` type.
    ///
    /// The data written to the returned encoder will be compressed and then
    /// written to the stream `w`.
    pub fn new(w: W, level: CompressionLevel) -> EncoderWriter<W> {
        Builder::new().writer(w, level)
    }

    /// Finish encoding this stream, returning the underlying writer once the
    /// encoding is done.
    pub fn finish(mut self) -> IoResult<W> {
        self.do_finish()
    }

    fn do_finish(&mut self) -> IoResult<W> {
        if self.header.len() != 0 {
            try!(self.inner.write(self.header.as_slice()));
        }
        try!(self.inner.do_finish());
        let mut inner = self.inner.inner.take().unwrap();
        try!(inner.write_le_u32(self.crc.sum() as u32));
        try!(inner.write_le_u32(self.crc.amt()));
        Ok(inner)
    }
}

impl<W: Writer> Writer for EncoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        if self.header.len() != 0 {
            try!(self.inner.inner.as_mut().unwrap().write(self.header.as_slice()));
            self.header.truncate(0);
        }
        try!(self.inner.write(buf));
        self.crc.update(buf);
        Ok(())
    }

    fn flush(&mut self) -> IoResult<()> { self.inner.flush() }
}

#[unsafe_destructor]
impl<W: Writer> Drop for EncoderWriter<W> {
    fn drop(&mut self) {
        if self.inner.inner.is_some() {
            let _ = self.do_finish();
        }
    }
}

impl<R: Reader> EncoderReader<R> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// The encoder is not configured specially for the emitted header. For
    /// header configuration, see the `Builder` type.
    ///
    /// The data read from the stream `r` will be compressed and available
    /// through the returned reader.
    pub fn new(r: R, level: CompressionLevel) -> EncoderReader<R> {
        Builder::new().reader(r, level)
    }

    /// Returns the underlying stream, consuming this encoder
    pub fn unwrap(self) -> R {
        self.inner.inner.unwrap()
    }

    fn read_footer(&mut self, into: &mut [u8]) -> IoResult<uint> {
        if self.pos == 8 {
            return Err(io::standard_error(io::EndOfFile))
        }
        let ref arr = [
            (self.inner.inner.crc().sum() >>  0) as u8,
            (self.inner.inner.crc().sum() >>  8) as u8,
            (self.inner.inner.crc().sum() >> 16) as u8,
            (self.inner.inner.crc().sum() >> 24) as u8,
            (self.inner.inner.crc().amt() >>  0) as u8,
            (self.inner.inner.crc().amt() >>  8) as u8,
            (self.inner.inner.crc().amt() >> 16) as u8,
            (self.inner.inner.crc().amt() >> 24) as u8,
        ];
        Ok(copy(into, arr, &mut self.pos))
    }
}

fn copy(into: &mut [u8], from: &[u8], pos: &mut uint) -> uint {
    let min = cmp::min(into.len(), from.len() - *pos);
    bytes::copy_memory(into, from.slice(*pos, *pos + min));
    *pos += min;
    return min
}

impl<R: Reader> Reader for EncoderReader<R> {
    fn read(&mut self, mut into: &mut [u8]) -> IoResult<uint> {
        let mut amt = 0;
        if self.eof {
            return self.read_footer(into)
        } else if self.pos < self.header.len() {
            amt += copy(into, self.header.as_slice(), &mut self.pos);
            if amt == into.len() { return Ok(amt) }
            let tmp = into; into = tmp.slice_from_mut(amt);
        }
        match self.inner.read(into) {
            Ok(a) => Ok(amt + a),
            Err(ref e) if e.kind == io::EndOfFile => {
                self.eof = true;
                self.pos = 0;
                self.read_footer(into)
            }
            Err(e) => Err(e)
        }
    }
}

impl<R: Reader> DecoderReader<R> {
    /// Creates a new decoder from the given reader, immediately parsing the
    /// gzip header.
    ///
    /// If an error is encountered when parsing the gzip header, an error is
    /// returned.
    pub fn new(r: R) -> IoResult<DecoderReader<R>> {
        // from here, all reads should go through this reader (not r):
        let mut crc_reader = CrcReader::new( r );
        
        let id1 = try!(crc_reader.read_u8());
        let id2 = try!(crc_reader.read_u8());
        if id1 != 0x1f || id2 != 0x8b { return Err(bad_header()) }
        let cm = try!(crc_reader.read_u8());
        if cm != 8 { return Err(bad_header()) }

        let flg = try!(crc_reader.read_u8());
        let mtime = try!(crc_reader.read_le_u32());
        let _xfl = try!(crc_reader.read_u8());
        let _os = try!(crc_reader.read_u8());

        let extra = if flg & FEXTRA != 0 {
            let xlen = try!(crc_reader.read_le_u16());
            Some(try!(crc_reader.read_exact(xlen as uint)))
        } else {
            None
        };
        let filename = if flg & FNAME != 0 {
            // wow this is slow
            let mut b = Vec::new();
            for byte in crc_reader.bytes() {
                let byte = try!(byte);
                if byte == 0 { break }
                b.push(byte);
            }
            Some(b)
        } else {
            None
        };
        let comment = if flg & FCOMMENT != 0 {
            // wow this is slow
            let mut b = Vec::new();
            for byte in crc_reader.bytes() {
                let byte = try!(byte);
                if byte == 0 { break }
                b.push(byte);
            }
            Some(b)
        } else {
            None
        };

        if flg & FHCRC != 0 {
            let calced_crc = crc_reader.crc().sum() & 0xFFFF;
            let stored_crc = try!(crc_reader.read_le_u16()) as libc::c_ulong;
            if calced_crc != stored_crc { return Err(corrupt()) }
        }

        let flate = raw::DecoderReader::new(crc_reader.unwrap(), true, Vec::with_capacity(128 * 1024));
        return Ok(DecoderReader {
            inner: CrcReader::new(flate),
            header: Header {
                extra: extra,
                filename: filename,
                comment: comment,
                mtime: mtime,
            }
        });

        fn bad_header() -> IoError {
            IoError {
                kind: io::InvalidInput,
                desc: "invalid gzip header",
                detail: None,
            }
        }
    }

    /// Returns the header associated with this stream.
    pub fn header(&self) -> &Header { &self.header }

    fn finish(&mut self) -> IoResult<()> {
        let ref mut buf = [0u8, ..8];
        {
            let flate = self.inner.inner();
            let len = {
                let remaining = flate.buf.slice_from(flate.buf.len());
                let len = cmp::min(remaining.len(), buf.len());
                bytes::copy_memory(buf, remaining.slice_to(len));
                len
            };

            if len < buf.len() {
                try!(flate.read_at_least(buf.len() - len, buf));
            }
        }

        let crc = (buf[0] as u32 <<  0) |
                  (buf[1] as u32 <<  8) |
                  (buf[2] as u32 << 16) |
                  (buf[3] as u32 << 24);
        let amt = (buf[4] as u32 <<  0) |
                  (buf[5] as u32 <<  8) |
                  (buf[6] as u32 << 16) |
                  (buf[7] as u32 << 24);
        if crc != self.inner.crc().sum() as u32 { return Err(corrupt()) }
        if amt != self.inner.crc().amt() { return Err(corrupt()) }
        Ok(())
    }
}

impl<R: Reader> Reader for DecoderReader<R> {
    fn read(&mut self, into: &mut [u8]) -> IoResult<uint> {
        match self.inner.read(into) {
            Ok(amt) => Ok(amt),
            Err(e) => {
                if e.kind == io::EndOfFile {
                    try!(self.finish());
                }
                return Err(e)
            }
        }
    }
}

impl Header {
    /// Returns the `filename` field of this gzip stream's header, if present.
    pub fn filename<'a>(&'a self) -> Option<&'a [u8]> {
        self.filename.as_ref().map(|s| s.as_slice())
    }
    /// Returns the `extra` field of this gzip stream's header, if present.
    pub fn extra<'a>(&'a self) -> Option<&'a [u8]> {
        self.extra.as_ref().map(|s| s.as_slice())
    }
    /// Returns the `comment` field of this gzip stream's header, if present.
    pub fn comment<'a>(&'a self) -> Option<&'a [u8]> {
        self.comment.as_ref().map(|s| s.as_slice())
    }
    /// Returns the `mtime` field of this gzip stream's header, if present.
    pub fn mtime(&self) -> u32 { self.mtime }
}

fn corrupt() -> IoError {
    IoError {
        kind: io::OtherIoError,
        desc: "corrupt gzip stream does not have a matching checksum",
        detail: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{EncoderWriter, EncoderReader, DecoderReader, Builder};
    use CompressionLevel::Default;
    use std::io::{MemWriter, MemReader};
    use std::rand::{task_rng, Rng};

    #[test]
    fn roundtrip() {
        let mut e = EncoderWriter::new(MemWriter::new(), Default);
        e.write(b"foo bar baz").unwrap();
        let inner = e.finish().unwrap();
        let mut d = DecoderReader::new(MemReader::new(inner.into_inner()));
        assert_eq!(d.read_to_string().unwrap().as_slice(), "foo bar baz");
    }

    #[test]
    fn roundtrip_big() {
        let mut real = Vec::new();
        let mut w = EncoderWriter::new(MemWriter::new(), Default);
        let v = task_rng().gen_iter::<u8>().take(1024).collect::<Vec<_>>();
        for _ in range(0u, 200) {
            let to_write = v.slice_to(task_rng().gen_range(0, v.len()));
            real.push_all(to_write);
            w.write(to_write).unwrap();
        }
        let result = w.finish().unwrap();
        let mut r = DecoderReader::new(MemReader::new(result.into_inner()));
        assert!(r.read_to_end().unwrap() == real);
    }

    #[test]
    fn roundtrip_big2() {
        let v = task_rng().gen_iter::<u8>().take(1024 * 1024).collect::<Vec<_>>();
        let r = MemReader::new(v.clone());
        let mut r = DecoderReader::new(EncoderReader::new(r, Default));
        assert!(r.read_to_end().unwrap() == v);
    }

    #[test]
    fn fields() {
        let r = MemReader::new(vec![0, 2, 4, 6]);
        let e = Builder::new().filename("foo.rs")
                              .comment("bar")
                              .extra(vec![0, 1, 2, 3])
                              .reader(r, Default);
        let mut d = DecoderReader::new(e).unwrap();
        assert_eq!(d.header().filename(), Some(b"foo.rs"));
        assert_eq!(d.header().comment(), Some(b"bar"));
        assert_eq!(d.header().extra(), Some(b"\x00\x01\x02\x03"));
        assert_eq!(d.read_to_end().unwrap(), vec![0, 2, 4, 6]);

    }
}
