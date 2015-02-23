use std::cmp;
use std::io::prelude::*;
use std::io;

/// Reader adaptor which limits the bytes read from an underlying reader.
///
/// For more information, see `ReadExt::take`.
pub struct Take<T> {
    pub inner: T,
    pub limit: u64,
}

impl<T: Read> Read for Take<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.limit == 0 { return Ok(0) }
        let max = cmp::min(buf.len() as u64, self.limit) as usize;
        let n = try!(self.inner.read(&mut buf[..max]));
        self.limit -= n as u64;
        Ok(n)
    }
}
