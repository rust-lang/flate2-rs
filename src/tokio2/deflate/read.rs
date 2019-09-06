use std::io;
use std::io::prelude::*;
use std::marker::Unpin;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project::pin_project;

use super::bufread;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

// use super::bufread;
// use crate::bufreader::BufReader;

/// A DEFLATE encoder, or compressor.
///
/// This structure implements a [`Read`] interface and will read uncompressed
/// data from an underlying stream and emit a stream of compressed data.
///
/// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// use flate2::Compression;
/// use flate2::read::DeflateEncoder;
///
/// # fn main() {
/// #    println!("{:?}", deflateencoder_read_hello_world().unwrap());
/// # }
/// #
/// // Return a vector containing the Deflate compressed version of hello world
/// fn deflateencoder_read_hello_world() -> io::Result<Vec<u8>> {
///    let mut ret_vec = [0;100];
///    let c = b"hello world";
///    let mut deflater = DeflateEncoder::new(&c[..], Compression::fast());
///    let count = deflater.read(&mut ret_vec)?;
///    Ok(ret_vec[0..count].to_vec())
/// }
/// ```
#[pin_project]
#[derive(Debug)]
pub struct DeflateEncoder<R: AsyncRead> {
    #[pin]
    inner: bufread::DeflateEncoder<BufReader<R>>,
}

impl<R: AsyncRead> DeflateEncoder<R> {
    /// Creates a new encoder which will read uncompressed data from the given
    /// stream and emit the compressed stream.
    pub fn new(r: R, level: crate::Compression) -> DeflateEncoder<R> {
        DeflateEncoder {
            inner: bufread::DeflateEncoder::new(BufReader::new(r), level),
        }
    }
}

impl<R: AsyncRead> DeflateEncoder<R> {
    /// Acquires a reference to the underlying reader
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this encoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    // /// Acquires a pinned mutable reference to the underlying reader that this encoder is wrapping.
    // ///
    // /// Note that care must be taken to avoid tampering with the state of the reader which may
    // /// otherwise confuse this encoder.
    // pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut bufread::DeflateEncoder<BufReader<R>>> {
    //     self.project().inner
    // }

    /// Consumes this encoder, returning the underlying reader.
    ///
    /// Note that there may be buffered bytes which are not re-acquired as part
    /// of this transition. It's recommended to only call this function after
    /// EOF has been reached.
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }

    /// Returns the number of bytes that have been read into this compressor.
    ///
    /// Note that not all bytes read from the underlying object may be accounted
    /// for, there may still be some active buffering.
    pub fn total_in(&self) -> u64 {
        self.inner.total_in()
    }

    /// Returns the number of bytes that the compressor has produced.
    ///
    /// Note that not all bytes may have been read yet, some may still be
    /// buffered.
    pub fn total_out(&self) -> u64 {
        self.inner.total_out()
    }
}

impl<R: AsyncRead> AsyncRead for DeflateEncoder<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_read(cx, buf)
    }
}

//
// impl<R: AsyncWrite + AsyncRead> AsyncWrite for DeflateEncoder<R> {
//     fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
//         self.project().inner.poll_write(cx, buf)
//     }

//     fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
//         AsyncWrite::poll_flush(Pin::new(self.get_mut().get_mut()), cx)
//     }

//     fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
//         AsyncWrite::poll_shutdown(Pin::new(self.get_mut().get_mut()), cx)
//     }
// }

/// A DEFLATE decoder, or decompressor.
///
/// This structure implements a [`Read`] interface and takes a stream of
/// compressed data as input, providing the decompressed data when read from.
///
/// [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// # use flate2::Compression;
/// # use flate2::write::DeflateEncoder;
/// use flate2::read::DeflateDecoder;
///
/// # fn main() {
/// #    let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
/// #    e.write_all(b"Hello World").unwrap();
/// #    let bytes = e.finish().unwrap();
/// #    println!("{}", decode_reader(bytes).unwrap());
/// # }
/// // Uncompresses a Deflate Encoded vector of bytes and returns a string or error
/// // Here &[u8] implements Read
/// fn decode_reader(bytes: Vec<u8>) -> io::Result<String> {
///    let mut deflater = DeflateDecoder::new(&bytes[..]);
///    let mut s = String::new();
///    deflater.read_to_string(&mut s)?;
///    Ok(s)
/// }
/// ```
#[pin_project]
#[derive(Debug)]
pub struct DeflateDecoder<R: AsyncRead> {
    #[pin]
    inner: bufread::DeflateDecoder<BufReader<R>>,
}

impl<R: AsyncRead> DeflateDecoder<R> {
    /// Creates a new decoder which will decompress data read from the given
    /// stream.
    pub fn new(r: R) -> DeflateDecoder<R> {
        DeflateDecoder::with_capacity(crate::DEFAULT_CAPACITY, r)
    }

    /// Same as `new`, but the intermediate buffer for data is specified.
    ///
    /// Note that the capacity of the intermediate buffer is never increased,
    /// and it is recommended for it to be large.
    pub fn with_capacity(capacity: usize, r: R) -> DeflateDecoder<R> {
        DeflateDecoder {
            inner: bufread::DeflateDecoder::new(BufReader::with_capacity(capacity, r)),
        }
    }
}

impl<R: AsyncRead> DeflateDecoder<R> {
    /// Acquires a reference to the underlying stream
    pub fn get_ref(&self) -> &R {
        self.inner.get_ref().get_ref()
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this encoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        self.inner.get_mut().get_mut()
    }

    /// Consumes this decoder, returning the underlying reader.
    ///
    /// Note that there may be buffered bytes which are not re-acquired as part
    /// of this transition. It's recommended to only call this function after
    /// EOF has been reached.
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }

    /// Returns the number of bytes that the decompressor has consumed.
    ///
    /// Note that this will likely be smaller than what the decompressor
    /// actually read from the underlying stream due to buffering.
    pub fn total_in(&self) -> u64 {
        self.inner.total_in()
    }

    /// Returns the number of bytes that the decompressor has produced.
    pub fn total_out(&self) -> u64 {
        self.inner.total_out()
    }
}

impl<R: AsyncRead> AsyncRead for DeflateDecoder<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_read(cx, buf)
    }
}

//
// impl<R: AsyncWrite + AsyncRead> AsyncWrite for DeflateDecoder<R> {
//     fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
//         AsyncWrite::poll_write(Pin::new(self.get_mut().get_mut()), cx, buf)
//     }

//     fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
//         AsyncWrite::poll_flush(Pin::new(self.get_mut().get_mut()), cx)
//     }

//     fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
//         AsyncWrite::poll_shutdown(Pin::new(self.get_mut().get_mut()), cx)
//     }
// }
