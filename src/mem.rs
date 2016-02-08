use std::error::Error;
use std::fmt;
use std::marker;
use std::mem;
use std::slice;

use libc::{c_int, c_uint};

use Compression;
use ffi;

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
    inner: Stream<DirCompress>,
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
    inner: Stream<DirDecompress>,
}

struct Stream<D: Direction> {
    raw: ffi::mz_stream,
    _marker: marker::PhantomData<D>,
}

unsafe impl<D: Direction> Send for Stream<D> {}
unsafe impl<D: Direction> Sync for Stream<D> {}

trait Direction {
    unsafe fn destroy(stream: *mut ffi::mz_stream) -> c_int;
}

enum DirCompress {}
enum DirDecompress {}

/// Values which indicate the form of flushing to be used when compressing or
/// decompressing in-memory data.
pub enum Flush {
    /// A typical parameter for passing to compression/decompression functions,
    /// this indicates that the underlying stream to decide how much data to
    /// accumulate before producing output in order to maximize compression.
    None = ffi::MZ_NO_FLUSH as isize,

    /// All pending output is flushed to the output buffer and the output is
    /// aligned on a byte boundary so that the decompressor can get all input
    /// data available so far.
    ///
    /// Flushing may degrade compression for some compression algorithms and so
    /// it should only be used when necessary. This will complete the current
    /// deflate block and follow it with an empty stored block.
    Sync = ffi::MZ_SYNC_FLUSH as isize,

    /// All pending output is flushed to the output buffer, but the output is
    /// not aligned to a byte boundary.
    ///
    /// All of the input data so far will be available to the decompressor (as
    /// with `Flush::Sync`. This completes the current deflate block and follows
    /// it with an empty fixed codes block that is 10 bites long, and it assures
    /// that enough bytes are output in order for the decompessor to finish the
    /// block before the empty fixed code block.
    Partial = ffi::MZ_PARTIAL_FLUSH as isize,

    /// A deflate block is completed and emitted, as for `Flush::Sync`, but the
    /// output is not aligned on a byte boundary and up to seven vits of the
    /// current block are held to be written as the next byte after the next
    /// deflate block is completed.
    ///
    /// In this case the decompressor may not be provided enough bits at this
    /// point in order to complete decompression of the data provided so far to
    /// the compressor, it may need to wait for the next block to be emitted.
    /// This is for advanced applications that need to control the emission of
    /// deflate blocks.
    Block = ffi::MZ_BLOCK as isize,

    /// All output is flushed as with `Flush::Sync` and the compression state is
    /// reset so decompression can restart from this point if previous
    /// compressed data has been damaged or if random access is desired.
    ///
    /// Using this option too often can seriously degrade compression.
    Full = ffi::MZ_FULL_FLUSH as isize,

    /// Pending input is processed and pending output is flushed.
    ///
    /// The return value may indicate that the stream is not yet done and more
    /// data has yet to be processed.
    Finish = ffi::MZ_FINISH as isize,
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
        unsafe {
            let mut state: ffi::mz_stream = mem::zeroed();
            let ret = ffi::mz_deflateInit2(&mut state,
                                           level as c_int,
                                           ffi::MZ_DEFLATED,
                                           if zlib_header {
                                               ffi::MZ_DEFAULT_WINDOW_BITS
                                           } else {
                                               -ffi::MZ_DEFAULT_WINDOW_BITS
                                           },
                                           9,
                                           ffi::MZ_DEFAULT_STRATEGY);
            debug_assert_eq!(ret, 0);
            Compress {
                inner: Stream {
                    raw: state,
                    _marker: marker::PhantomData,
                },
            }
        }
    }

    /// Returns the total number of input bytes which have been processed by
    /// this compression object.
    pub fn total_in(&self) -> u64 {
        self.inner.raw.total_in as u64
    }

    /// Returns the total number of output bytes which have been produced by
    /// this compression object.
    pub fn total_out(&self) -> u64 {
        self.inner.raw.total_out as u64
    }

    /// Quickly resets this compressor without having to reallocate anything.
    ///
    /// This is equivalent to dropping this object and then creating a new one.
    pub fn reset(&mut self) {
        let rc = unsafe { ffi::mz_deflateReset(&mut self.inner.raw) };
        assert_eq!(rc, ffi::MZ_OK);
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
        self.inner.raw.next_in = input.as_ptr() as *mut _;
        self.inner.raw.avail_in = input.len() as c_uint;
        self.inner.raw.next_out = output.as_mut_ptr();
        self.inner.raw.avail_out = output.len() as c_uint;
        unsafe {
            match ffi::mz_deflate(&mut self.inner.raw, flush as c_int) {
                ffi::MZ_OK => Status::Ok,
                ffi::MZ_BUF_ERROR => Status::BufError,
                ffi::MZ_STREAM_END => Status::StreamEnd,
                c => panic!("unknown return code: {}", c),
            }
        }
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
        let cap = output.capacity();
        let len = output.len();

        unsafe {
            let before = self.total_out();
            let ret = {
                let ptr = output.as_mut_ptr().offset(len as isize);
                let out = slice::from_raw_parts_mut(ptr, cap - len);
                self.compress(input, out, flush)
            };
            output.set_len((self.total_out() - before) as usize + len);
            return ret
        }
    }
}

impl Decompress {
    /// Creates a new object ready for decompressing data that it's given.
    ///
    /// The `zlib_header` argument indicates whether the input data is expected
    /// to have a zlib header or not.
    pub fn new(zlib_header: bool) -> Decompress {
        unsafe {
            let mut state: ffi::mz_stream = mem::zeroed();
            let ret = ffi::mz_inflateInit2(&mut state,
                                           if zlib_header {
                                               ffi::MZ_DEFAULT_WINDOW_BITS
                                           } else {
                                               -ffi::MZ_DEFAULT_WINDOW_BITS
                                           });
            debug_assert_eq!(ret, 0);
            Decompress {
                inner: Stream {
                    raw: state,
                    _marker: marker::PhantomData,
                },
            }
        }
    }

    /// Returns the total number of input bytes which have been processed by
    /// this decompression object.
    pub fn total_in(&self) -> u64 {
        self.inner.raw.total_in as u64
    }

    /// Returns the total number of output bytes which have been produced by
    /// this decompression object.
    pub fn total_out(&self) -> u64 {
        self.inner.raw.total_out as u64
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
        self.inner.raw.next_in = input.as_ptr() as *mut u8;
        self.inner.raw.avail_in = input.len() as c_uint;
        self.inner.raw.next_out = output.as_mut_ptr();
        self.inner.raw.avail_out = output.len() as c_uint;
        unsafe {
            match ffi::mz_inflate(&mut self.inner.raw, flush as c_int) {
                ffi::MZ_DATA_ERROR => Err(DataError(())),
                ffi::MZ_OK => Ok(Status::Ok),
                ffi::MZ_BUF_ERROR => Ok(Status::BufError),
                ffi::MZ_STREAM_END => Ok(Status::StreamEnd),
                c => panic!("unknown return code: {}", c),
            }
        }
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
        let cap = output.capacity();
        let len = output.len();

        unsafe {
            let before = self.total_out();
            let ret = {
                let ptr = output.as_mut_ptr().offset(len as isize);
                let out = slice::from_raw_parts_mut(ptr, cap - len);
                self.decompress(input, out, flush)
            };
            output.set_len((self.total_out() - before) as usize + len);
            return ret
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

impl Direction for DirCompress {
    unsafe fn destroy(stream: *mut ffi::mz_stream) -> c_int {
        ffi::mz_deflateEnd(stream)
    }
}
impl Direction for DirDecompress {
    unsafe fn destroy(stream: *mut ffi::mz_stream) -> c_int {
        ffi::mz_inflateEnd(stream)
    }
}

impl<D: Direction> Drop for Stream<D> {
    fn drop(&mut self) {
        unsafe {
            let _ = D::destroy(&mut self.raw);
        }
    }
}
