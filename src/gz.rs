//! gzip compression/decompression
//!
//! [1]: http://www.gzip.org/zlib/rfc-gzip.html

use std::cmp;
use std::env;
use std::ffi::CString;
use std::io::prelude::*;
use std::io;
use std::mem;

use Compression;
use crc::{CrcReader, Crc};
use raw;

static FHCRC: u8 = 1 << 1;
static FEXTRA: u8 = 1 << 2;
static FNAME: u8 = 1 << 3;
static FCOMMENT: u8 = 1 << 4;

/// A gzip streaming encoder
///
/// This structure exposes a `Write` interface that will emit compressed data
/// to the underlying writer `W`.
pub struct EncoderWriter<W: Write> {
    inner: raw::EncoderWriter<W>,
    crc: Crc,
    header: Vec<u8>,
}

/// A gzip streaming encoder
///
/// This structure exposes a `Read` interface that will read uncompressed data
/// from the underlying reader and expose the compressed version as a `Read`
/// interface.
pub struct EncoderReader<R: Read> {
    inner: raw::EncoderReader<CrcReader<R>>,
    header: Vec<u8>,
    pos: usize,
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
/// This structure exposes a `Read` interface that will consume compressed
/// data from the underlying reader and emit uncompressed data.
pub struct DecoderReader<R: Read> {
    inner: CrcReader<raw::DecoderReader<R>>,
    header: Header,
    finished: bool,
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
    pub fn filename(mut self, filename: &[u8]) -> Builder {
        self.filename = Some(CString::new(filename).unwrap());
        self
    }

    /// Configure the `comment` field in the gzip header.
    pub fn comment(mut self, comment: &[u8]) -> Builder {
        self.comment = Some(CString::new(comment).unwrap());
        self
    }

    /// Consume this builder, creating a writer encoder in the process.
    ///
    /// The data written to the returned encoder will be compressed and then
    /// written out to the supplied parameter `w`.
    pub fn write<W: Write>(self, w: W, lvl: Compression) -> EncoderWriter<W> {
        EncoderWriter {
            inner: raw::EncoderWriter::new(w,
                                           lvl,
                                           true,
                                           Vec::with_capacity(32 * 1024)),
            crc: Crc::new(),
            header: self.into_header(lvl),
        }
    }

    /// Consume this builder, creating a reader encoder in the process.
    ///
    /// Data read from the returned encoder will be the compressed version of
    /// the data read from the given reader.
    pub fn read<R: Read>(self, r: R, lvl: Compression) -> EncoderReader<R> {
        let crc = CrcReader::new(r);
        EncoderReader {
            inner: raw::EncoderReader::new(crc, lvl, true, vec![0; 32 * 1024]),
            header: self.into_header(lvl),
            pos: 0,
            eof: false,
        }
    }

    fn into_header(self, lvl: Compression) -> Vec<u8> {
        let Builder { extra, filename, comment, mtime } = self;
        let mut flg = 0;
        let mut header = vec![0u8; 10];
        match extra {
            Some(v) => {
                flg |= FEXTRA;
                header.push((v.len() >> 0) as u8);
                header.push((v.len() >> 8) as u8);
                header.extend(v);
            }
            None => {}
        }
        match filename {
            Some(filename) => {
                flg |= FNAME;
                header.extend(filename.as_bytes_with_nul().iter().map(|x| *x));
            }
            None => {}
        }
        match comment {
            Some(comment) => {
                flg |= FCOMMENT;
                header.extend(comment.as_bytes_with_nul().iter().map(|x| *x));
            }
            None => {}
        }
        header[0] = 0x1f;
        header[1] = 0x8b;
        header[2] = 8;
        header[3] = flg;
        header[4] = (mtime >> 0) as u8;
        header[5] = (mtime >> 8) as u8;
        header[6] = (mtime >> 16) as u8;
        header[7] = (mtime >> 24) as u8;
        header[8] = match lvl {
            Compression::Best => 2,
            Compression::Fast => 4,
            _ => 0,
        };
        header[9] = match env::consts::OS {
            "linux" => 3,
            "macos" => 7,
            "win32" => 0,
            _ => 255,
        };
        return header;
    }
}

impl<W: Write> EncoderWriter<W> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// The encoder is not configured specially for the emitted header. For
    /// header configuration, see the `Builder` type.
    ///
    /// The data written to the returned encoder will be compressed and then
    /// written to the stream `w`.
    pub fn new(w: W, level: Compression) -> EncoderWriter<W> {
        Builder::new().write(w, level)
    }

    /// Finish encoding this stream, returning the underlying writer once the
    /// encoding is done.
    pub fn finish(mut self) -> io::Result<W> {
        self.do_finish()
    }

    fn do_finish(&mut self) -> io::Result<W> {
        if self.header.len() != 0 {
            try!(self.inner.write_all_raw(&self.header));
        }
        try!(self.inner.finish());
        let mut inner = self.inner.take_inner();
        let (sum, amt) = (self.crc.sum() as u32, self.crc.amt_as_u32());
        let buf = [(sum >> 0) as u8,
                   (sum >> 8) as u8,
                   (sum >> 16) as u8,
                   (sum >> 24) as u8,
                   (amt >> 0) as u8,
                   (amt >> 8) as u8,
                   (amt >> 16) as u8,
                   (amt >> 24) as u8];
        try!(inner.write_all(&buf));
        Ok(inner)
    }
}

impl<W: Write> Write for EncoderWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.header.len() != 0 {
            try!(self.inner.write_all_raw(&self.header));
            self.header.truncate(0);
        }
        let n = try!(self.inner.write(buf));
        self.crc.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Write> Drop for EncoderWriter<W> {
    fn drop(&mut self) {
        if !self.inner.unwrapped() {
            let _ = self.do_finish();
        }
    }
}

impl<R: Read> EncoderReader<R> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// The encoder is not configured specially for the emitted header. For
    /// header configuration, see the `Builder` type.
    ///
    /// The data read from the stream `r` will be compressed and available
    /// through the returned reader.
    pub fn new(r: R, level: Compression) -> EncoderReader<R> {
        Builder::new().read(r, level)
    }

    /// Returns the underlying stream, consuming this encoder
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }

    fn read_footer(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if self.pos == 8 {
            return Ok(0);
        }
        let crc = self.inner.get_ref().crc();
        let ref arr = [(crc.sum() >> 0) as u8,
                       (crc.sum() >> 8) as u8,
                       (crc.sum() >> 16) as u8,
                       (crc.sum() >> 24) as u8,
                       (crc.amt_as_u32() >> 0) as u8,
                       (crc.amt_as_u32() >> 8) as u8,
                       (crc.amt_as_u32() >> 16) as u8,
                       (crc.amt_as_u32() >> 24) as u8];
        Ok(copy(into, arr, &mut self.pos))
    }
}

fn copy(into: &mut [u8], from: &[u8], pos: &mut usize) -> usize {
    let min = cmp::min(into.len(), from.len() - *pos);
    for (slot, val) in into.iter_mut().zip(from[*pos..*pos + min].iter()) {
        *slot = *val;
    }
    *pos += min;
    return min;
}

impl<R: Read> Read for EncoderReader<R> {
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
        match try!(self.inner.read(into)) {
            0 => {
                self.eof = true;
                self.pos = 0;
                self.read_footer(into)
            }
            n => Ok(amt + n),
        }
    }
}

impl<R: Read> DecoderReader<R> {
    /// Creates a new decoder from the given reader, immediately parsing the
    /// gzip header.
    ///
    /// If an error is encountered when parsing the gzip header, an error is
    /// returned.
    pub fn new(r: R) -> io::Result<DecoderReader<R>> {
        let mut crc_reader = CrcReader::new(r);
        let mut header = [0; 10];
        try!(fill(&mut crc_reader, &mut header));

        let id1 = header[0];
        let id2 = header[1];
        if id1 != 0x1f || id2 != 0x8b {
            return Err(bad_header());
        }
        let cm = header[2];
        if cm != 8 {
            return Err(bad_header());
        }

        let flg = header[3];
        let mtime = ((header[4] as u32) << 0) | ((header[5] as u32) << 8) |
                    ((header[6] as u32) << 16) |
                    ((header[7] as u32) << 24);
        let _xfl = header[8];
        let _os = header[9];

        let extra = if flg & FEXTRA != 0 {
            let xlen = try!(read_le_u16(&mut crc_reader));
            let mut extra = vec![0; xlen as usize];
            try!(fill(&mut crc_reader, &mut extra));
            Some(extra)
        } else {
            None
        };
        let filename = if flg & FNAME != 0 {
            // wow this is slow
            let mut b = Vec::new();
            for byte in crc_reader.by_ref().bytes() {
                let byte = try!(byte);
                if byte == 0 {
                    break;
                }
                b.push(byte);
            }
            Some(b)
        } else {
            None
        };
        let comment = if flg & FCOMMENT != 0 {
            // wow this is slow
            let mut b = Vec::new();
            for byte in crc_reader.by_ref().bytes() {
                let byte = try!(byte);
                if byte == 0 {
                    break;
                }
                b.push(byte);
            }
            Some(b)
        } else {
            None
        };

        if flg & FHCRC != 0 {
            let calced_crc = crc_reader.crc().sum() as u16;
            let stored_crc = try!(read_le_u16(&mut crc_reader));
            if calced_crc != stored_crc {
                return Err(corrupt());
            }
        }

        let flate = raw::DecoderReader::new(crc_reader.into_inner(),
                                            true,
                                            vec![0; 32 * 1024]);
        return Ok(DecoderReader {
            inner: CrcReader::new(flate),
            header: Header {
                extra: extra,
                filename: filename,
                comment: comment,
                mtime: mtime,
            },
            finished: false,
        });

        fn bad_header() -> io::Error {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid gzip header")
        }

        fn fill<R: Read>(r: &mut R, mut buf: &mut [u8]) -> io::Result<()> {
            while buf.len() > 0 {
                match try!(r.read(buf)) {
                    0 => return Err(corrupt()),
                    n => buf = &mut mem::replace(&mut buf, &mut [])[n..],
                }
            }
            Ok(())
        }

        fn read_le_u16<R: Read>(r: &mut R) -> io::Result<u16> {
            let mut b = [0; 2];
            try!(fill(r, &mut b));
            Ok((b[0] as u16) | ((b[1] as u16) << 8))
        }
    }

    /// Returns the header associated with this stream.
    pub fn header(&self) -> &Header {
        &self.header
    }

    fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        let ref mut buf = [0u8; 8];
        {
            let mut len = 0;

            while len < buf.len() {
                match try!(self.inner.inner().read_raw(&mut buf[len..])) {
                    0 => return Err(corrupt()),
                    n => len += n,
                }
            }
        }

        let crc = ((buf[0] as u32) << 0) | ((buf[1] as u32) << 8) |
                  ((buf[2] as u32) << 16) |
                  ((buf[3] as u32) << 24);
        let amt = ((buf[4] as u32) << 0) | ((buf[5] as u32) << 8) |
                  ((buf[6] as u32) << 16) |
                  ((buf[7] as u32) << 24);
        if crc != self.inner.crc().sum() as u32 {
            return Err(corrupt());
        }
        if amt != self.inner.crc().amt_as_u32() {
            return Err(corrupt());
        }
        self.finished = true;
        Ok(())
    }
}

impl<R: Read> Read for DecoderReader<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        match try!(self.inner.read(into)) {
            0 => {
                try!(self.finish());
                Ok(0)
            }
            n => Ok(n),
        }
    }
}

impl Header {
    /// Returns the `filename` field of this gzip stream's header, if present.
    pub fn filename(&self) -> Option<&[u8]> {
        self.filename.as_ref().map(|s| &s[..])
    }

    /// Returns the `extra` field of this gzip stream's header, if present.
    pub fn extra(&self) -> Option<&[u8]> {
        self.extra.as_ref().map(|s| &s[..])
    }

    /// Returns the `comment` field of this gzip stream's header, if present.
    pub fn comment(&self) -> Option<&[u8]> {
        self.comment.as_ref().map(|s| &s[..])
    }

    /// Returns the `mtime` field of this gzip stream's header, if present.
    pub fn mtime(&self) -> u32 {
        self.mtime
    }
}

fn corrupt() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput,
                   "corrupt gzip stream does not have a matching checksum")
}

#[cfg(test)]
mod tests {
    use std::io::prelude::*;

    use super::{EncoderWriter, EncoderReader, DecoderReader, Builder};
    use Compression::Default;
    use rand::{thread_rng, Rng};

    #[test]
    fn roundtrip() {
        let mut e = EncoderWriter::new(Vec::new(), Default);
        e.write_all(b"foo bar baz").unwrap();
        let inner = e.finish().unwrap();
        let mut d = DecoderReader::new(&inner[..]).unwrap();
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "foo bar baz");
    }

    #[test]
    fn roundtrip_zero() {
        let e = EncoderWriter::new(Vec::new(), Default);
        let inner = e.finish().unwrap();
        let mut d = DecoderReader::new(&inner[..]).unwrap();
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn roundtrip_big() {
        let mut real = Vec::new();
        let mut w = EncoderWriter::new(Vec::new(), Default);
        let v = thread_rng().gen_iter::<u8>().take(1024).collect::<Vec<_>>();
        for _ in 0..200 {
            let to_write = &v[..thread_rng().gen_range(0, v.len())];
            real.extend(to_write.iter().map(|x| *x));
            w.write_all(to_write).unwrap();
        }
        let result = w.finish().unwrap();
        let mut r = DecoderReader::new(&result[..]).unwrap();
        let mut v = Vec::new();
        r.read_to_end(&mut v).unwrap();
        assert!(v == real);
    }

    #[test]
    fn roundtrip_big2() {
        let v = thread_rng()
                    .gen_iter::<u8>()
                    .take(1024 * 1024)
                    .collect::<Vec<_>>();
        let mut r = DecoderReader::new(EncoderReader::new(&v[..], Default))
                        .unwrap();
        let mut res = Vec::new();
        r.read_to_end(&mut res).unwrap();
        assert!(res == v);
    }

    #[test]
    fn fields() {
        let r = vec![0, 2, 4, 6];
        let e = Builder::new()
                    .filename(b"foo.rs")
                    .comment(b"bar")
                    .extra(vec![0, 1, 2, 3])
                    .read(&r[..], Default);
        let mut d = DecoderReader::new(e).unwrap();
        assert_eq!(d.header().filename(), Some(&b"foo.rs"[..]));
        assert_eq!(d.header().comment(), Some(&b"bar"[..]));
        assert_eq!(d.header().extra(), Some(&b"\x00\x01\x02\x03"[..]));
        let mut res = Vec::new();
        d.read_to_end(&mut res).unwrap();
        assert_eq!(res, vec![0, 2, 4, 6]);

    }

    #[test]
    fn keep_reading_after_end() {
        let mut e = EncoderWriter::new(Vec::new(), Default);
        e.write_all(b"foo bar baz").unwrap();
        let inner = e.finish().unwrap();
        let mut d = DecoderReader::new(&inner[..]).unwrap();
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "foo bar baz");
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "foo bar baz");
    }
}
