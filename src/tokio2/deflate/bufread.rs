use std::io;
use std::io::prelude::*;
use std::marker::Unpin;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project::pin_project;
use std::future::Future;

use futures::ready;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite};

use crate::zio::{Flush, Ops};
use crate::{Compress, Decompress};

/// A DEFLATE encoder, or compressor.
///
/// This structure implements a [`BufRead`] interface and will read uncompressed
/// data from an underlying stream and emit a stream of compressed data.
///
/// [`BufRead`]: https://doc.rust-lang.org/std/io/trait.BufRead.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// use flate2::Compression;
/// use flate2::bufread::DeflateEncoder;
/// use std::fs::File;
/// use std::io::BufReader;
///
/// # fn main() {
/// #    println!("{:?}", open_hello_world().unwrap());
/// # }
/// #
/// // Opens sample file, compresses the contents and returns a Vector
/// fn open_hello_world() -> io::Result<Vec<u8>> {
///    let f = File::open("examples/hello_world.txt")?;
///    let b = BufReader::new(f);
///    let mut deflater = DeflateEncoder::new(b, Compression::fast());
///    let mut buffer = Vec::new();
///    deflater.read_to_end(&mut buffer)?;
///    Ok(buffer)
/// }
/// ```
///
#[pin_project]
#[derive(Debug)]
pub struct DeflateEncoder<R: AsyncBufRead> {
    #[pin]
    obj: R,
    flushing: bool,
    data: Compress,
}

impl<R: AsyncBufRead> DeflateEncoder<R> {
    /// Creates a new encoder which will read uncompressed data from the given
    /// stream and emit the compressed stream.
    pub fn new(r: R, level: crate::Compression) -> DeflateEncoder<R> {
        DeflateEncoder {
            obj: r,
            flushing: false,
            data: Compress::new(level, false),
        }
    }
}

impl<R: AsyncBufRead> DeflateEncoder<R> {
    /// Acquires a reference to the underlying reader
    pub fn get_ref(&self) -> &R {
        &self.obj
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this encoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.obj
    }

    /// Consumes this encoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.obj
    }

    /// Returns the number of bytes that have been read into this compressor.
    ///
    /// Note that not all bytes read from the underlying object may be accounted
    /// for, there may still be some active buffering.
    pub fn total_in(&self) -> u64 {
        self.data.total_in()
    }

    /// Returns the number of bytes that the compressor has produced.
    ///
    /// Note that not all bytes may have been read yet, some may still be
    /// buffered.
    pub fn total_out(&self) -> u64 {
        self.data.total_out()
    }
}

impl<R: AsyncBufRead> AsyncRead for DeflateEncoder<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut this = self.project();

        loop {
            let input_buffer = ready!(this.obj.as_mut().poll_fill_buf(cx))?;
            *this.flushing = input_buffer.is_empty();

            let flush = if *this.flushing {
                <Compress as Ops>::Flush::finish()
            } else {
                <Compress as Ops>::Flush::none()
            };

            let (prior_in, prior_out) = (this.data.total_in(), this.data.total_out());
            this.data.compress(input_buffer, buf, flush)?;
            let input = this.data.total_in() - prior_in;
            let output = this.data.total_out() - prior_out;

            this.obj.as_mut().consume(input as usize);
            if *this.flushing || output > 0 {
                return Poll::Ready(Ok(output as usize));
            }
        }
    }
}

impl<R: AsyncWrite + AsyncBufRead> AsyncWrite for DeflateEncoder<R> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        this.obj.poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.project();
        *this.flushing = true;
        this.obj.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.project();
        this.obj.poll_shutdown(cx)
    }
}

/// A DEFLATE decoder, or decompressor.
///
/// This structure implements a [`BufRead`] interface and takes a stream of
/// compressed data as input, providing the decompressed data when read from.
///
/// [`BufRead`]: https://doc.rust-lang.org/std/io/trait.BufRead.html
///
/// # Examples
///
/// ```
/// use std::io::prelude::*;
/// use std::io;
/// # use flate2::Compression;
/// # use flate2::write::DeflateEncoder;
/// use flate2::bufread::DeflateDecoder;
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
pub struct DeflateDecoder<R: AsyncBufRead> {
    #[pin]
    obj: R,
    flushing: bool,
    data: Decompress,
}

impl<R: AsyncBufRead> DeflateDecoder<R> {
    /// Creates a new decoder which will decompress data read from the given
    /// stream.
    pub fn new(r: R) -> DeflateDecoder<R> {
        DeflateDecoder {
            obj: r,
            flushing: false,
            data: Decompress::new(false),
        }
    }
}

impl<R: AsyncBufRead> DeflateDecoder<R> {
    /// Acquires a reference to the underlying stream
    pub fn get_ref(&self) -> &R {
        &self.obj
    }

    /// Acquires a mutable reference to the underlying stream
    ///
    /// Note that mutation of the stream may result in surprising results if
    /// this encoder is continued to be used.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.obj
    }

    /// Consumes this decoder, returning the underlying reader.
    pub fn into_inner(self) -> R {
        self.obj
    }

    /// Returns the number of bytes that the decompressor has consumed.
    ///
    /// Note that this will likely be smaller than what the decompressor
    /// actually read from the underlying stream due to buffering.
    pub fn total_in(&self) -> u64 {
        self.data.total_in()
    }

    /// Returns the number of bytes that the decompressor has produced.
    pub fn total_out(&self) -> u64 {
        self.data.total_out()
    }
}

impl<R: AsyncRead + AsyncBufRead> AsyncRead for DeflateDecoder<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut this = self.project();

        loop {
            let input_buffer = ready!(this.obj.as_mut().poll_fill_buf(cx))?;
            *this.flushing = input_buffer.is_empty();

            let flush = if *this.flushing {
                <Decompress as Ops>::Flush::finish()
            } else {
                <Decompress as Ops>::Flush::none()
            };

            let (prior_in, prior_out) = (this.data.total_in(), this.data.total_out());
            this.data.decompress(input_buffer, buf, flush)?;
            let input = this.data.total_in() - prior_in;
            let output = this.data.total_out() - prior_out;

            this.obj.as_mut().consume(input as usize);
            if *this.flushing || output > 0 {
                return Poll::Ready(Ok(output as usize));
            }
        }
    }
}

impl<R: AsyncWrite + AsyncBufRead + Unpin> AsyncWrite for DeflateDecoder<R> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(self.get_mut().get_mut()), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(self.get_mut().get_mut()), cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        AsyncWrite::poll_shutdown(Pin::new(self.get_mut().get_mut()), cx)
    }
}
