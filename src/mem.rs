use std::error::Error;
use std::fmt;

use libc::c_int;

use {Compression, Flush};
use ffi;
use stream::{self, Stream};

/// Raw in-memory compression stream for blocks of data.
///
/// This type is the building block for the I/O streams in the rest of this
/// crate. It requires more management than the `Read`/`Write` API but is
/// maximally flexible in terms of accepting input from any source and being
/// able to produce output to any memory location.
///
/// It is recommended to use the I/O stream adaptors over this type as they're
/// easier to use.
pub struct Compress {
    inner: Stream<stream::Compress>,
}

/// Raw in-memory decompression stream for blocks of data.
///
/// This type is the building block for the I/O streams in the rest of this
/// crate. It requires more management than the `Read`/`Write` API but is
/// maximally flexible in terms of accepting input from any source and being
/// able to produce output to any memory location.
///
/// It is recommended to use the I/O stream adaptors over this type as they're
/// easier to use.
pub struct Decompress {
    inner: Stream<stream::Decompress>,
}

/// Error returned when a decompression object finds that the input stream of
/// bytes was not a valid input stream of bytes.
#[derive(Debug)]
pub struct DataError(());

/// Possible status results of compressing some data or successfully
/// decompressing a block of data.
pub enum Status {
    /// Indicates success.
    ///
    /// Means that more input may be needed but isn't available
    /// and/or there' smore output to be written but the output buffer is full.
    Ok,

    /// Indicates that forward progress is not possible due to input or output
    /// buffers being empty.
    ///
    /// For compression it means the input buffer needs some more data or the
    /// output buffer needs to be freed up before trying again.
    ///
    /// For decompression this means that more input is needed to continue or
    /// the output buffer isn't large enough to contain the result. The function
    /// can be called again after fixing both.
    BufError,

    /// Indicates that all input has been consumed and all output bytes have
    /// been written. Decompression/compression should not be called again.
    ///
    /// For decompression with zlib streams the adler-32 of the decompressed
    /// data has also been verified.
    StreamEnd,
}

impl Compress {
    /// Creates a new object ready for compressing data that it's given.
    ///
    /// The `level` argument here indicates what level of compression is going
    /// to be performed, and the `zlib_header` argument indicates whether the
    /// output data should have a zlib header or not.
    pub fn new(level: Compression, zlib_header: bool) -> Compress {
        Compress { inner: Stream::new_compress(level, !zlib_header) }
    }

    /// Returns the total number of input bytes which have been processed by
    /// this compression object.
    pub fn total_in(&self) -> u64 {
        self.inner.total_in()
    }

    /// Returns the total number of output bytes which have been produced by
    /// this compression object.
    pub fn total_out(&self) -> u64 {
        self.inner.total_out()
    }

    /// Quickly resets this compressor without having to reallocate anything.
    ///
    /// This is equivalent to dropping this object and then creating a new one.
    pub fn reset(&mut self) {
        assert_eq!(self.inner.reset(), ffi::MZ_OK);
    }

    /// Compresses the input data into the output, consuming only as much
    /// input as needed and writing as much output as possible.
    ///
    /// The flush option can be any of the available flushing parameters.
    ///
    /// To learn how much data was consumed or how much output was produced, use
    /// the `total_in` and `total_out` functions before/after this is called.
    pub fn compress(&mut self,
                    input: &[u8],
                    output: &mut [u8],
                    flush: Flush)
                    -> Status {
        let rc = self.inner.compress(input, output, flush);
        self.rc(rc)
    }

    /// Compresses the input data into the extra space of the output, consuming
    /// only as much input as needed and writing as much output as possible.
    ///
    /// This function has the same semantics as `compress`, except that the
    /// length of `vec` is managed by this function. This will not reallocate
    /// the vector provided or attempt to grow it, so space for the output must
    /// be reserved in the output vector by the caller before calling this
    /// function.
    pub fn compress_vec(&mut self,
                        input: &[u8],
                        output: &mut Vec<u8>,
                        flush: Flush)
                        -> Status {
        let rc = self.inner.compress_vec(input, output, flush);
        self.rc(rc)
    }

    fn rc(&self, rc: c_int) -> Status {
        match rc {
            ffi::MZ_OK => Status::Ok,
            ffi::MZ_BUF_ERROR => Status::BufError,
            ffi::MZ_STREAM_END => Status::StreamEnd,
            c => panic!("unknown return code: {}", c),
        }
    }
}

impl Decompress {
    /// Creates a new object ready for decompressing data that it's given.
    ///
    /// The `zlib_header` argument indicates whether the input data is expected
    /// to have a zlib header or not.
    pub fn new(zlib_header: bool) -> Decompress {
        Decompress { inner: Stream::new_decompress(!zlib_header) }
    }

    /// Returns the total number of input bytes which have been processed by
    /// this decompression object.
    pub fn total_in(&self) -> u64 {
        self.inner.total_in()
    }

    /// Returns the total number of output bytes which have been produced by
    /// this decompression object.
    pub fn total_out(&self) -> u64 {
        self.inner.total_out()
    }

    /// Decompresses the input data into the output, consuming only as much
    /// input as needed and writing as much output as possible.
    ///
    /// The flush option provided can either be `Flush::None`, `Flush::Sync`,
    /// or `Flush::Finish`. If the first call passes `Flush::Finish` it is
    /// assumed that the input and output buffers are both sized large enough to
    /// decompress the entire stream in a single call.
    ///
    /// A flush value of `Flush::Finish` indicates that there are no more source
    /// bytes available beside what's already in the input buffer, and the
    /// output buffer is large enough to hold the rest of the decompressed data.
    ///
    /// To learn how much data was consumed or how much output was produced, use
    /// the `total_in` and `total_out` functions before/after this is called.
    pub fn decompress(&mut self,
                      input: &[u8],
                      output: &mut [u8],
                      flush: Flush)
                      -> Result<Status, DataError> {
        let rc = self.inner.decompress(input, output, flush);
        self.rc(rc)
    }

    /// Decompresses the input data into the extra space in the output vector
    /// specified by `output`.
    ///
    /// This function has the same semantics as `decompress`, except that the
    /// length of `vec` is managed by this function. This will not reallocate
    /// the vector provided or attempt to grow it, so space for the output must
    /// be reserved in the output vector by the caller before calling this
    /// function.
    pub fn decompress_vec(&mut self,
                          input: &[u8],
                          output: &mut Vec<u8>,
                          flush: Flush)
                          -> Result<Status, DataError> {
        let rc = self.inner.decompress_vec(input, output, flush);
        self.rc(rc)
    }

    fn rc(&self, rc: c_int) -> Result<Status, DataError> {
        match rc {
            ffi::MZ_DATA_ERROR => Err(DataError(())),
            ffi::MZ_OK => Ok(Status::Ok),
            ffi::MZ_BUF_ERROR => Ok(Status::BufError),
            ffi::MZ_STREAM_END => Ok(Status::StreamEnd),
            c => panic!("unknown return code: {}", c),
        }
    }
}

impl Error for DataError {
    fn description(&self) -> &str { "deflate data error" }
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.description().fmt(f)
    }
}
