use std::ffi::CString;
use std::io::{self, prelude::*};
use std::time;

use crate::bufreader::BufReader;
use crate::{Compression, Crc};

pub static FHCRC: u8 = 1 << 1;
pub static FEXTRA: u8 = 1 << 2;
pub static FNAME: u8 = 1 << 3;
pub static FCOMMENT: u8 = 1 << 4;

pub mod bufread;
pub mod read;
pub mod write;

/// A structure representing the header of a gzip stream.
///
/// The header can contain metadata about the file that was compressed, if
/// present.
#[derive(PartialEq, Clone, Debug, Default)]
pub struct GzHeader {
    extra: Option<Vec<u8>>,
    filename: Option<Vec<u8>>,
    comment: Option<Vec<u8>>,
    operating_system: u8,
    mtime: u32,
}

impl GzHeader {
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

    /// Returns the `operating_system` field of this gzip stream's header.
    ///
    /// There are predefined values for various operating systems.
    /// 255 means that the value is unknown.
    pub fn operating_system(&self) -> u8 {
        self.operating_system
    }

    /// This gives the most recent modification time of the original file being compressed.
    ///
    /// The time is in Unix format, i.e., seconds since 00:00:00 GMT, Jan. 1, 1970.
    /// (Note that this may cause problems for MS-DOS and other systems that use local
    /// rather than Universal time.) If the compressed data did not come from a file,
    /// `mtime` is set to the time at which compression started.
    /// `mtime` = 0 means no time stamp is available.
    ///
    /// The usage of `mtime` is discouraged because of Year 2038 problem.
    pub fn mtime(&self) -> u32 {
        self.mtime
    }

    /// Returns the most recent modification time represented by a date-time type.
    /// Returns `None` if the value of the underlying counter is 0,
    /// indicating no time stamp is available.
    ///
    ///
    /// The time is measured as seconds since 00:00:00 GMT, Jan. 1 1970.
    /// See [`mtime`](#method.mtime) for more detail.
    pub fn mtime_as_datetime(&self) -> Option<time::SystemTime> {
        if self.mtime == 0 {
            None
        } else {
            let duration = time::Duration::new(u64::from(self.mtime), 0);
            let datetime = time::UNIX_EPOCH + duration;
            Some(datetime)
        }
    }
}

#[derive(Debug)]
pub enum GzHeaderParsingState {
    Start,
    Xlen,
    Extra,
    Filename,
    Comment,
    Crc,
}

#[derive(Debug)]
pub struct GzHeaderPartial {
    buf: Vec<u8>,
    state: GzHeaderParsingState,
    flg: u8,
    xlen: u16,
    crc: Crc,
    header: GzHeader,
}

impl GzHeaderPartial {
    fn new() -> GzHeaderPartial {
        GzHeaderPartial {
            buf: Vec::with_capacity(10), // minimum header length
            state: GzHeaderParsingState::Start,
            flg: 0,
            xlen: 0,
            crc: Crc::new(),
            header: GzHeader {
                extra: None,
                filename: None,
                comment: None,
                operating_system: 0,
                mtime: 0,
            },
        }
    }

    pub fn take_header(self) -> GzHeader {
        self.header
    }
}

fn read_gz_header_part<'a, R: Read>(r: &'a mut Buffer<'a, R>) -> io::Result<()> {
    loop {
        match r.part.state {
            GzHeaderParsingState::Start => {
                let mut header = [0; 10];
                r.read_and_forget(&mut header)?;

                if header[0] != 0x1f || header[1] != 0x8b {
                    return Err(bad_header());
                }
                if header[2] != 8 {
                    return Err(bad_header());
                }

                r.part.flg = header[3];
                r.part.header.mtime = ((header[4] as u32) << 0)
                    | ((header[5] as u32) << 8)
                    | ((header[6] as u32) << 16)
                    | ((header[7] as u32) << 24);
                let _xfl = header[8];
                r.part.header.operating_system = header[9];
                r.part.state = GzHeaderParsingState::Xlen;
            }
            GzHeaderParsingState::Xlen => {
                if r.part.flg & FEXTRA != 0 {
                    r.part.xlen = read_le_u16(r)?;
                }
                r.part.state = GzHeaderParsingState::Extra;
            }
            GzHeaderParsingState::Extra => {
                if r.part.flg & FEXTRA != 0 {
                    let mut extra = vec![0; r.part.xlen as usize];
                    r.read_and_forget(&mut extra)?;
                    r.part.header.extra = Some(extra);
                }
                r.part.state = GzHeaderParsingState::Filename;
            }
            GzHeaderParsingState::Filename => {
                if r.part.flg & FNAME != 0 {
                    if r.part.header.filename.is_none() {
                        r.part.header.filename = Some(Vec::new());
                    };
                    for byte in r.bytes() {
                        let byte = byte?;
                        if byte == 0 {
                            break;
                        }
                    }
                }
                r.part.state = GzHeaderParsingState::Comment;
            }
            GzHeaderParsingState::Comment => {
                if r.part.flg & FCOMMENT != 0 {
                    if r.part.header.comment.is_none() {
                        r.part.header.comment = Some(Vec::new());
                    };
                    for byte in r.bytes() {
                        let byte = byte?;
                        if byte == 0 {
                            break;
                        }
                    }
                }
                r.part.state = GzHeaderParsingState::Crc;
            }
            GzHeaderParsingState::Crc => {
                if r.part.flg & FHCRC != 0 {
                    let stored_crc = read_le_u16(r)?;
                    let calced_crc = r.part.crc.sum() as u16;
                    if stored_crc != calced_crc {
                        return Err(corrupt());
                    }
                }
                return Ok(());
            }
        }
    }
}

fn read_gz_header<R: Read>(r: &mut R) -> io::Result<GzHeader> {
    let mut part = GzHeaderPartial::new();

    let result = {
        let mut reader = Buffer::new(&mut part, r);
        read_gz_header_part(&mut reader)
    };
    result.map(|()| part.take_header())
}

fn read_le_u16<R: Read>(r: &mut Buffer<R>) -> io::Result<u16> {
    let mut b = [0; 2];
    r.read_and_forget(&mut b)?;
    Ok((b[0] as u16) | ((b[1] as u16) << 8))
}

fn bad_header() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid gzip header")
}

fn corrupt() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "corrupt gzip stream does not have a matching checksum",
    )
}

/// A builder structure to create a new gzip Encoder.
///
/// This structure controls header configuration options such as the filename.
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// # use std::io;
/// use std::fs::File;
/// use flate2::GzBuilder;
/// use flate2::Compression;
///
/// // GzBuilder opens a file and writes a sample string using GzBuilder pattern
///
/// # fn sample_builder() -> Result<(), io::Error> {
/// let f = File::create("examples/hello_world.gz")?;
/// let mut gz = GzBuilder::new()
///                 .filename("hello_world.txt")
///                 .comment("test file, please delete")
///                 .write(f, Compression::default());
/// gz.write_all(b"hello world")?;
/// gz.finish()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct GzBuilder {
    extra: Option<Vec<u8>>,
    filename: Option<CString>,
    comment: Option<CString>,
    operating_system: Option<u8>,
    mtime: u32,
}

impl Default for GzBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GzBuilder {
    /// Create a new blank builder with no header by default.
    pub fn new() -> GzBuilder {
        GzBuilder {
            extra: None,
            filename: None,
            comment: None,
            operating_system: None,
            mtime: 0,
        }
    }

    /// Configure the `mtime` field in the gzip header.
    pub fn mtime(mut self, mtime: u32) -> GzBuilder {
        self.mtime = mtime;
        self
    }

    /// Configure the `operating_system` field in the gzip header.
    pub fn operating_system(mut self, os: u8) -> GzBuilder {
        self.operating_system = Some(os);
        self
    }

    /// Configure the `extra` field in the gzip header.
    pub fn extra<T: Into<Vec<u8>>>(mut self, extra: T) -> GzBuilder {
        self.extra = Some(extra.into());
        self
    }

    /// Configure the `filename` field in the gzip header.
    ///
    /// # Panics
    ///
    /// Panics if the `filename` slice contains a zero.
    pub fn filename<T: Into<Vec<u8>>>(mut self, filename: T) -> GzBuilder {
        self.filename = Some(CString::new(filename.into()).unwrap());
        self
    }

    /// Configure the `comment` field in the gzip header.
    ///
    /// # Panics
    ///
    /// Panics if the `comment` slice contains a zero.
    pub fn comment<T: Into<Vec<u8>>>(mut self, comment: T) -> GzBuilder {
        self.comment = Some(CString::new(comment.into()).unwrap());
        self
    }

    /// Consume this builder, creating a writer encoder in the process.
    ///
    /// The data written to the returned encoder will be compressed and then
    /// written out to the supplied parameter `w`.
    pub fn write<W: Write>(self, w: W, lvl: Compression) -> write::GzEncoder<W> {
        write::gz_encoder(self.into_header(lvl), w, lvl)
    }

    /// Consume this builder, creating a reader encoder in the process.
    ///
    /// Data read from the returned encoder will be the compressed version of
    /// the data read from the given reader.
    pub fn read<R: Read>(self, r: R, lvl: Compression) -> read::GzEncoder<R> {
        read::gz_encoder(self.buf_read(BufReader::new(r), lvl))
    }

    /// Consume this builder, creating a reader encoder in the process.
    ///
    /// Data read from the returned encoder will be the compressed version of
    /// the data read from the given reader.
    pub fn buf_read<R>(self, r: R, lvl: Compression) -> bufread::GzEncoder<R>
    where
        R: BufRead,
    {
        bufread::gz_encoder(self.into_header(lvl), r, lvl)
    }

    fn into_header(self, lvl: Compression) -> Vec<u8> {
        let GzBuilder {
            extra,
            filename,
            comment,
            operating_system,
            mtime,
        } = self;
        let mut flg = 0;
        let mut header = vec![0u8; 10];
        if let Some(v) = extra {
            flg |= FEXTRA;
            header.push((v.len() >> 0) as u8);
            header.push((v.len() >> 8) as u8);
            header.extend(v);
        }
        if let Some(filename) = filename {
            flg |= FNAME;
            header.extend(filename.as_bytes_with_nul().iter().copied());
        }
        if let Some(comment) = comment {
            flg |= FCOMMENT;
            header.extend(comment.as_bytes_with_nul().iter().copied());
        }
        header[0] = 0x1f;
        header[1] = 0x8b;
        header[2] = 8;
        header[3] = flg;
        header[4] = (mtime >> 0) as u8;
        header[5] = (mtime >> 8) as u8;
        header[6] = (mtime >> 16) as u8;
        header[7] = (mtime >> 24) as u8;
        header[8] = if lvl.0 >= Compression::best().0 {
            2
        } else if lvl.0 <= Compression::fast().0 {
            4
        } else {
            0
        };

        // Typically this byte indicates what OS the gz stream was created on,
        // but in an effort to have cross-platform reproducible streams just
        // default this value to 255. I'm not sure that if we "correctly" set
        // this it'd do anything anyway...
        header[9] = operating_system.unwrap_or(255);
        header
    }
}

/// A small adapter which reads data originally from `buf` and then reads all
/// further data from `reader`. This will also buffer all data read from
/// `reader` into `buf` for reuse on a further call.
struct Buffer<'a, T: 'a> {
    part: &'a mut GzHeaderPartial,
    buf_cur: usize,
    buf_max: usize,
    reader: &'a mut T,
}

impl<'a, T> Buffer<'a, T> {
    fn new(part: &'a mut GzHeaderPartial, reader: &'a mut T) -> Buffer<'a, T> {
        Buffer {
            reader,
            buf_cur: 0,
            buf_max: part.buf.len(),
            part,
        }
    }
}

impl<'a, T: Read> Read for Buffer<'a, T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut bufref = match self.part.state {
            GzHeaderParsingState::Filename => self.part.header.filename.as_mut(),
            GzHeaderParsingState::Comment => self.part.header.comment.as_mut(),
            _ => None,
        };
        if let Some(ref mut b) = bufref {
            // we have a direct reference to a buffer where to write
            let len = self.reader.read(buf)?;
            if len > 0 && buf[len - 1] == 0 {
                // we do not append the final 0
                b.extend_from_slice(&buf[..len - 1]);
            } else {
                b.extend_from_slice(&buf[..len]);
            }
            self.part.crc.update(&buf[..len]);
            Ok(len)
        } else if self.buf_cur == self.buf_max {
            // we read new bytes and also save them in self.part.buf
            let len = self.reader.read(buf)?;
            self.part.buf.extend_from_slice(&buf[..len]);
            self.part.crc.update(&buf[..len]);
            Ok(len)
        } else {
            // we first read the previously saved bytes
            let len = (&self.part.buf[self.buf_cur..self.buf_max]).read(buf)?;
            self.buf_cur += len;
            Ok(len)
        }
    }
}

impl<'a, T> Buffer<'a, T>
where
    T: std::io::Read,
{
    // If we manage to read all the bytes, we reset the buffer
    fn read_and_forget(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_exact(buf)?;
        // we managed to read the whole buf
        // we will no longer need the previously saved bytes in self.part.buf
        let rlen = buf.len();
        self.part.buf.truncate(0);
        self.buf_cur = 0;
        self.buf_max = 0;
        Ok(rlen)
    }
}

#[cfg(test)]
mod tests {
    use std::io::prelude::*;

    use super::{read, write, GzBuilder};
    use crate::Compression;
    use rand::{thread_rng, Rng};

    #[test]
    fn roundtrip() {
        let mut e = write::GzEncoder::new(Vec::new(), Compression::default());
        e.write_all(b"foo bar baz").unwrap();
        let inner = e.finish().unwrap();
        let mut d = read::GzDecoder::new(&inner[..]);
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "foo bar baz");
    }

    #[test]
    fn roundtrip_zero() {
        let e = write::GzEncoder::new(Vec::new(), Compression::default());
        let inner = e.finish().unwrap();
        let mut d = read::GzDecoder::new(&inner[..]);
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn roundtrip_big() {
        let mut real = Vec::new();
        let mut w = write::GzEncoder::new(Vec::new(), Compression::default());
        let v = crate::random_bytes().take(1024).collect::<Vec<_>>();
        for _ in 0..200 {
            let to_write = &v[..thread_rng().gen_range(0..v.len())];
            real.extend(to_write.iter().copied());
            w.write_all(to_write).unwrap();
        }
        let result = w.finish().unwrap();
        let mut r = read::GzDecoder::new(&result[..]);
        let mut v = Vec::new();
        r.read_to_end(&mut v).unwrap();
        assert_eq!(v, real);
    }

    #[test]
    fn roundtrip_big2() {
        let v = crate::random_bytes().take(1024 * 1024).collect::<Vec<_>>();
        let mut r = read::GzDecoder::new(read::GzEncoder::new(&v[..], Compression::default()));
        let mut res = Vec::new();
        r.read_to_end(&mut res).unwrap();
        assert_eq!(res, v);
    }

    #[test]
    fn fields() {
        let r = vec![0, 2, 4, 6];
        let e = GzBuilder::new()
            .filename("foo.rs")
            .comment("bar")
            .extra(vec![0, 1, 2, 3])
            .read(&r[..], Compression::default());
        let mut d = read::GzDecoder::new(e);
        assert_eq!(d.header().unwrap().filename(), Some(&b"foo.rs"[..]));
        assert_eq!(d.header().unwrap().comment(), Some(&b"bar"[..]));
        assert_eq!(d.header().unwrap().extra(), Some(&b"\x00\x01\x02\x03"[..]));
        let mut res = Vec::new();
        d.read_to_end(&mut res).unwrap();
        assert_eq!(res, vec![0, 2, 4, 6]);
    }

    #[test]
    fn keep_reading_after_end() {
        let mut e = write::GzEncoder::new(Vec::new(), Compression::default());
        e.write_all(b"foo bar baz").unwrap();
        let inner = e.finish().unwrap();
        let mut d = read::GzDecoder::new(&inner[..]);
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "foo bar baz");
        d.read_to_string(&mut s).unwrap();
        assert_eq!(s, "foo bar baz");
    }

    #[test]
    fn qc_reader() {
        ::quickcheck::quickcheck(test as fn(_) -> _);

        fn test(v: Vec<u8>) -> bool {
            let r = read::GzEncoder::new(&v[..], Compression::default());
            let mut r = read::GzDecoder::new(r);
            let mut v2 = Vec::new();
            r.read_to_end(&mut v2).unwrap();
            v == v2
        }
    }

    #[test]
    fn flush_after_write() {
        let mut f = write::GzEncoder::new(Vec::new(), Compression::default());
        write!(f, "Hello world").unwrap();
        f.flush().unwrap();
    }

    use crate::gz::bufread::tests::BlockingCursor;
    #[test]
    // test function read_and_forget of Buffer
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
        let mut decoder = read::GzDecoder::new(r);
        let mut out = Vec::with_capacity(7);
        match decoder.read(&mut out) {
            Err(e) => {
                assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
            }
            _ => {
                panic!("Unexpected result for decoder.read");
            }
        }
    }
}
