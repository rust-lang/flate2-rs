use std::io;
use std::io::BufRead;

use {Decompress, Compress, Status, Flush, DataError};

pub trait ReadData {
    fn total_in(&self) -> u64;
    fn total_out(&self) -> u64;
    fn run(&mut self, input: &[u8], output: &mut [u8], flush: Flush)
           -> Result<Status, DataError>;
}

impl ReadData for Compress {
    fn total_in(&self) -> u64 { self.total_in() }
    fn total_out(&self) -> u64 { self.total_out() }
    fn run(&mut self, input: &[u8], output: &mut [u8], flush: Flush)
           -> Result<Status, DataError> {
        Ok(self.compress(input, output, flush))
    }
}

impl ReadData for Decompress {
    fn total_in(&self) -> u64 { self.total_in() }
    fn total_out(&self) -> u64 { self.total_out() }
    fn run(&mut self, input: &[u8], output: &mut [u8], flush: Flush)
           -> Result<Status, DataError> {
        self.decompress(input, output, flush)
    }
}

pub fn read<R, D>(obj: &mut R, data: &mut D, dst: &mut [u8]) -> io::Result<usize>
    where R: BufRead, D: ReadData
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
