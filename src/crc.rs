//! Simple CRC bindings backed by miniz.c

use std::io::prelude::*;
use std::io;
use libc;

use ffi;

pub struct Crc {
    crc: libc::c_ulong,
    amt: u32,
}

pub struct CrcReader<R> {
    inner: R,
    crc: Crc,
}

impl Crc {
    pub fn new() -> Crc {
        Crc { crc: 0, amt: 0 }
    }

    pub fn sum(&self) -> libc::c_ulong {
        self.crc
    }

    pub fn amt_as_u32(&self) -> u32 {
        self.amt
    }

    pub fn update(&mut self, data: &[u8]) {
        self.amt = self.amt.wrapping_add(data.len() as u32);
        self.crc = unsafe {
            ffi::mz_crc32(self.crc, data.as_ptr(), data.len() as libc::size_t)
        };
    }
}

impl<R: Read> CrcReader<R> {
    pub fn new(r: R) -> CrcReader<R> {
        CrcReader {
            inner: r,
            crc: Crc::new(),
        }
    }

    pub fn crc(&self) -> &Crc {
        &self.crc
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    pub fn inner(&mut self) -> &mut R {
        &mut self.inner
    }
}

impl<R: Read> Read for CrcReader<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let amt = try!(self.inner.read(into));
        self.crc.update(&into[..amt]);
        Ok(amt)
    }
}
