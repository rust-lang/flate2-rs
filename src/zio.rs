use std::io::prelude::*;
use std::io;
use std::mem;

use {Decompress, Compress, Status, Flush, DataError};

pub struct Writer<W: Write, D: Ops> {
    obj: Option<W>,
    pub data: D,
    buf: Vec<u8>,
}

pub trait Ops {
    fn total_in(&self) -> u64;
    fn total_out(&self) -> u64;
    fn run(&mut self, input: &[u8], output: &mut [u8], flush: Flush)
           -> Result<Status, DataError>;
    fn run_vec(&mut self, input: &[u8], output: &mut Vec<u8>, flush: Flush)
               -> Result<Status, DataError>;
}

impl Ops for Compress {
    fn total_in(&self) -> u64 { self.total_in() }
    fn total_out(&self) -> u64 { self.total_out() }
    fn run(&mut self, input: &[u8], output: &mut [u8], flush: Flush)
           -> Result<Status, DataError> {
        Ok(self.compress(input, output, flush))
    }
    fn run_vec(&mut self, input: &[u8], output: &mut Vec<u8>, flush: Flush)
               -> Result<Status, DataError> {
        Ok(self.compress_vec(input, output, flush))
    }
}

impl Ops for Decompress {
    fn total_in(&self) -> u64 { self.total_in() }
    fn total_out(&self) -> u64 { self.total_out() }
    fn run(&mut self, input: &[u8], output: &mut [u8], flush: Flush)
           -> Result<Status, DataError> {
        self.decompress(input, output, flush)
    }
    fn run_vec(&mut self, input: &[u8], output: &mut Vec<u8>, flush: Flush)
               -> Result<Status, DataError> {
        self.decompress_vec(input, output, flush)
    }
}

pub fn read<R, D>(obj: &mut R, data: &mut D, dst: &mut [u8]) -> io::Result<usize>
    where R: BufRead, D: Ops
{
    loop {
        let (read, consumed, ret, eof);
        {
            let input = try!(obj.fill_buf());
            eof = input.is_empty();
            let before_out = data.total_out();
            let before_in = data.total_in();
            let flush = if eof {Flush::Finish} else {Flush::None};
            ret = data.run(input, dst, flush);
            read = (data.total_out() - before_out) as usize;
            consumed = (data.total_in() - before_in) as usize;
        }
        obj.consume(consumed);

        match ret {
            // If we haven't ready any data and we haven't hit EOF yet,
            // then we need to keep asking for more data because if we
            // return that 0 bytes of data have been read then it will
            // be interpreted as EOF.
            Ok(Status::Ok) |
            Ok(Status::BufError) if read == 0 && !eof && dst.len() > 0 => {
                continue
            }
            Ok(Status::Ok) |
            Ok(Status::BufError) |
            Ok(Status::StreamEnd) => return Ok(read),

            Err(..) => return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                                 "corrupt deflate stream"))
        }
    }
}

impl<W: Write, D: Ops> Writer<W, D> {
    pub fn new(w: W, d: D) -> Writer<W, D> {
        Writer {
            obj: Some(w),
            data: d,
            buf: Vec::with_capacity(32 * 1024),
        }
    }

    pub fn finish(&mut self) -> io::Result<()> {
        loop {
            try!(self.dump());

            let before = self.data.total_out();
            self.data.run_vec(&[], &mut self.buf, Flush::Finish).unwrap();
            if before == self.data.total_out() {
                return Ok(())
            }
        }
    }

    pub fn replace(&mut self, w: W) -> W {
        self.buf.truncate(0);
        mem::replace(&mut self.obj, Some(w)).unwrap()
    }

    pub fn get_mut(&mut self) -> Option<&mut W> {
        self.obj.as_mut()
    }

    pub fn take_inner(&mut self) -> Option<W> {
        self.obj.take()
    }

    pub fn into_inner(mut self) -> W {
        self.take_inner().unwrap()
    }

    fn dump(&mut self) -> io::Result<()> {
        if self.buf.len() > 0 {
            try!(self.obj.as_mut().unwrap().write_all(&self.buf));
            self.buf.truncate(0);
        }
        Ok(())
    }
}

impl<W: Write, D: Ops> Write for Writer<W, D> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // miniz isn't guaranteed to actually write any of the buffer provided,
        // it may be in a flushing mode where it's just giving us data before
        // we're actually giving it any data. We don't want to spuriously return
        // `Ok(0)` when possible as it will cause calls to write_all() to fail.
        // As a result we execute this in a loop to ensure that we try our
        // darndest to write the data.
        loop {
            try!(self.dump());

            let before_in = self.data.total_in();
            let ret = self.data.run_vec(buf, &mut self.buf, Flush::None);
            let written = (self.data.total_in() - before_in) as usize;

            if buf.len() > 0 && written == 0 && ret.is_ok() {
                continue
            }
            return match ret {
                Ok(Status::Ok) |
                Ok(Status::BufError) |
                Ok(Status::StreamEnd) => Ok(written),

                Err(..) => Err(io::Error::new(io::ErrorKind::InvalidInput,
                                              "corrupt deflate stream"))
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // Unfortunately miniz doesn't actually tell us when we're done with
        // pulling out all the data from the internal stream. To remedy this we
        // have to continually ask the stream for more memory until it doesn't
        // give us a chunk of memory the same size as our own internal buffer,
        // at which point we assume it's reached the end.
        loop {
            try!(self.dump());

            let before = self.data.total_out();
            self.data.run_vec(&[], &mut self.buf, Flush::Sync).unwrap();
            if before == self.data.total_out() {
                break
            }
        }

        self.obj.as_mut().unwrap().flush()
    }
}

impl<W: Write, D: Ops> Drop for Writer<W, D> {
    fn drop(&mut self) {
        if self.obj.is_some() {
            let _ = self.finish();
        }
    }
}
