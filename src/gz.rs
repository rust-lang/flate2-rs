//! gzip compression/decompression
//!
//! [1]: http://www.gzip.org/zlib/rfc-gzip.html

use libc;
use std::c_str::CString;
use std::cmp;
use std::io::{IoResult, IoError};
use std::io;
use std::os;
use std::slice::bytes;
use std::u16;

use {BestCompression, CompressionLevel, BestSpeed};
use ffi;

static FHCRC: u8 = 1 << 1;
static FEXTRA: u8 = 1 << 2;
static FNAME: u8 = 1 << 3;
static FCOMMENT: u8 = 1 << 4;

/// A gzip streaming encoder
///
/// This structure exposes a `Writer` interface that will emit compressed data
/// to the underlying writer `W`.
pub struct Encoder<W> {
    inner: ::EncoderWriter<W>,
    crc: libc::c_ulong,
    amt: u32,
    extra: Option<Vec<u8>>,
    filename: Option<CString>,
    comment: Option<CString>,
    wrote_header: bool,
    mtime: u32,
    xfl: u8,
}

/// A gzip streaming decoder
///
/// This structure exposes a `Reader` interface that will consume compressed
/// data from the underlying reader and emit uncompressed data.
pub struct Decoder<R> {
    inner: ::DecoderReader<R>,
    crc: libc::c_ulong,
    amt: u32,
    extra: Option<Vec<u8>>,
    filename: Option<Vec<u8>>,
    comment: Option<Vec<u8>>,
    mtime: u32,
}

impl<W: Writer> Encoder<W> {
    /// Creates a new encoder which will use the given compression level.
    ///
    /// No data is written at this time, and the gzip header can be configured
    /// before the first call to `write()` by invoking the other instance
    /// methods of this encoder.
    pub fn new(w: W, level: CompressionLevel) -> Encoder<W> {
        Encoder {
            inner: ::EncoderWriter::new(w, level, true, Vec::with_capacity(128 * 1024)),
            crc: 0,
            amt: 0,
            wrote_header: false,
            extra: None,
            filename: None,
            comment: None,
            mtime: 0,
            xfl: match level {
                BestCompression => 2,
                BestSpeed => 4,
                _ => 0,
            }
        }
    }

    /// Configure the `mtime` field in the gzip header.
    ///
    /// This function will return an error if the header has already been
    /// written.
    pub fn mtime(&mut self, mtime: u32) -> IoResult<()> {
        if self.wrote_header {
            Err(io::standard_error(io::OtherIoError))
        } else {
            self.mtime = mtime;
            Ok(())
        }
    }

    /// Configure the `extra` field in the gzip header.
    ///
    /// This function will return an error if the header has already been
    /// written.
    pub fn extra(&mut self, extra: Vec<u8>) -> IoResult<()> {
        if self.wrote_header || extra.len() >= u16::MAX as uint {
            Err(io::standard_error(io::OtherIoError))
        } else {
            self.extra = Some(extra);
            Ok(())
        }
    }

    /// Configure the `filename` field in the gzip header.
    ///
    /// This function will return an error if the header has already been
    /// written.
    pub fn filename<T: ToCStr>(&mut self, filename: T) -> IoResult<()> {
        if self.wrote_header {
            Err(io::standard_error(io::OtherIoError))
        } else {
            self.filename = Some(filename.to_c_str());
            Ok(())
        }
    }

    /// Configure the `comment` field in the gzip header.
    ///
    /// This function will return an error if the header has already been
    /// written.
    pub fn comment<T: ToCStr>(&mut self, comment: T) -> IoResult<()> {
        if self.wrote_header {
            Err(io::standard_error(io::OtherIoError))
        } else {
            self.comment = Some(comment.to_c_str());
            Ok(())
        }
    }

    fn write_header(&mut self) -> IoResult<()> {
        let w = self.inner.inner.get_mut_ref();
        try!(w.write_u8(0x1f));
        try!(w.write_u8(0x8b));
        try!(w.write_u8(8));
        let flg = if self.extra.is_some() {FEXTRA} else {0} |
                  if self.filename.is_some() {FNAME} else {0} |
                  if self.comment.is_some() {FCOMMENT} else {0};
        try!(w.write_u8(flg));
        try!(w.write_le_u32(self.mtime));
        try!(w.write_u8(self.xfl));
        try!(w.write_u8(match os::consts::SYSNAME {
            "linux" => 3,
            "macos" => 7,
            "win32" => 0,
            _ => 255,
        }));

        match self.extra {
            Some(ref vec) => {
                try!(w.write_le_u16(vec.len() as u16));
                try!(w.write(vec.as_slice()));
            }
            None => {}
        }
        match self.filename {
            Some(ref cstr) => try!(w.write(cstr.as_bytes())),
            None => {}
        }
        match self.comment {
            Some(ref cstr) => try!(w.write(cstr.as_bytes())),
            None => {}
        }
        Ok(())
    }

    /// Finish encoding this stream, returning the underlying writer once the
    /// encoding is done.
    pub fn finish(mut self) -> IoResult<W> {
        self.do_finish()
    }

    fn do_finish(&mut self) -> IoResult<W> {
        if !self.wrote_header {
            try!(self.write_header());
        }
        try!(self.inner.do_finish());
        let mut inner = self.inner.inner.take().unwrap();
        try!(inner.write_le_u32(self.crc as u32));
        try!(inner.write_le_u32(self.amt));
        Ok(inner)
    }
}

impl<W: Writer> Writer for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        if !self.wrote_header {
            self.wrote_header = true;
            try!(self.write_header());
        }
        try!(self.inner.write(buf));
        self.crc = unsafe {
            ffi::mz_crc32(self.crc, buf.as_ptr(), buf.len() as libc::size_t)
        };
        self.amt += buf.len() as u32;
        Ok(())
    }

    fn flush(&mut self) -> IoResult<()> { self.inner.flush() }
}

#[unsafe_destructor]
impl<W: Writer> Drop for Encoder<W> {
    fn drop(&mut self) {
        if self.inner.inner.is_some() {
            let _ = self.do_finish();
        }
    }
}

impl<R: Reader> Decoder<R> {
    /// Creates a new decoder from the given reader, immediately parsing the
    /// gzip header.
    ///
    /// If an error is encountered when parsing the gzip header, an error is
    /// returned.
    pub fn new(mut r: R) -> IoResult<Decoder<R>> {
        let id1 = try!(r.read_u8());
        let id2 = try!(r.read_u8());
        if id1 != 0x1f || id2 != 0x8b { return Err(bad_header()) }
        let cm = try!(r.read_u8());
        if cm != 8 { return Err(bad_header()) }

        let flg = try!(r.read_u8());
        let mtime = try!(r.read_le_u32());
        let _xfl = try!(r.read_u8());
        let _os = try!(r.read_u8());

        let extra = if flg & FEXTRA != 0 {
            let xlen = try!(r.read_le_u16());
            Some(try!(r.read_exact(xlen as uint)))
        } else {
            None
        };
        let filename = if flg & FNAME != 0 {
            // wow this is slow
            let mut b = Vec::new();
            for byte in r.bytes() {
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
            for byte in r.bytes() {
                let byte = try!(byte);
                if byte == 0 { break }
                b.push(byte);
            }
            Some(b)
        } else {
            None
        };

        if flg & FHCRC != 0 {
            try!(r.read_le_u16());
        }

        return Ok(Decoder {
            inner: ::DecoderReader::new(r, true, Vec::with_capacity(128 * 1024)),
            crc: 0,
            amt: 0,
            extra: extra,
            filename: filename,
            comment: comment,
            mtime: mtime,
        });

        fn bad_header() -> IoError {
            IoError {
                kind: io::InvalidInput,
                desc: "invalid gzip header",
                detail: None,
            }
        }
    }

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

    fn finish(&mut self) -> IoResult<()> {
        let mut buf = [0u8, ..8];
        let remaining = self.inner.buf.slice_from(self.inner.buf.len());
        let len = cmp::min(remaining.len(), buf.len());
        bytes::copy_memory(buf, remaining.slice_to(len));

        if len < buf.len() {
            try!(self.inner.inner.read_at_least(buf.len() - len, buf));
        }

        let crc = (buf[0] as u32 <<  0) |
                  (buf[1] as u32 <<  8) |
                  (buf[2] as u32 << 16) |
                  (buf[3] as u32 << 24);
        let amt = (buf[4] as u32 <<  0) |
                  (buf[5] as u32 <<  8) |
                  (buf[6] as u32 << 16) |
                  (buf[7] as u32 << 24);
        if crc != self.crc as u32 { return Err(corrupt()) }
        if amt != self.amt { return Err(corrupt()) }
        Ok(())
    }
}

impl<R: Reader> Reader for Decoder<R> {
    fn read(&mut self, into: &mut [u8]) -> IoResult<uint> {
        let amt = match self.inner.read(into) {
            Ok(amt) => amt,
            Err(e) => {
                if e.kind == io::EndOfFile {
                    try!(self.finish());
                }
                return Err(e)
            }
        };
        self.amt += amt as u32;
        self.crc = unsafe {
            ffi::mz_crc32(self.crc, into.as_ptr(), amt as libc::size_t)
        };
        Ok(amt)
    }
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
    use super::{Encoder, Decoder};
    use {Default};
    use std::io::{MemWriter, MemReader};
    use std::rand::{task_rng, Rng};

    #[test]
    fn roundtrip() {
        let mut e = Encoder::new(MemWriter::new(), Default);
        e.write(b"foo bar baz").unwrap();
        let inner = e.finish().unwrap();
        let mut d = Decoder::new(MemReader::new(inner.unwrap()));
        assert_eq!(d.read_to_string().unwrap().as_slice(), "foo bar baz");
    }

    #[test]
    fn roundtrip_big() {
        let mut real = Vec::new();
        let mut w = Encoder::new(MemWriter::new(), Default);
        let v = task_rng().gen_iter::<u8>().take(1024).collect::<Vec<_>>();
        for _ in range(0u, 200) {
            let to_write = v.slice_to(task_rng().gen_range(0, v.len()));
            real.push_all(to_write);
            w.write(to_write).unwrap();
        }
        let result = w.finish().unwrap();
        let mut r = Decoder::new(MemReader::new(result.unwrap()));
        assert!(r.read_to_end().unwrap() == real);
    }
}
