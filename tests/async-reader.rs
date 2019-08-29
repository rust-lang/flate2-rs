// extern crate flate2;
// extern crate tokio;

// use flate2::read::{GzDecoder, MultiGzDecoder};
// // use futures::prelude::*;
// // use futures::task;
// use std::cmp;
// use std::fs::File;
// use std::future::Future;
// use std::io::{self, Read};
// use std::pin::Pin;
// use std::task::{Context, Poll};
// use tokio::io::{AsyncRead, AsyncReadExt};
// // use tokio::ioAsyncRead;
// // use tokio::ioio::read_to_end;

// struct BadReader<T> {
//     reader: T,
//     x: bool,
// }

// impl<T> BadReader<T> {
//     fn new(reader: T) -> BadReader<T> {
//         BadReader { reader, x: true }
//     }
// }

// impl<T: Read> Read for BadReader<T> {
//     fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
//         if self.x {
//             self.x = false;
//             let len = cmp::min(buf.len(), 1);
//             self.reader.read(&mut buf[..len])
//         } else {
//             self.x = true;
//             Err(io::ErrorKind::WouldBlock.into())
//         }
//     }
// }

// impl<T: Read + Unpin> AsyncRead for BadReader<T> {
//     fn poll_read(
//         self: Pin<&mut Self>,
//         cx: &mut Context,
//         buf: &mut [u8],
//     ) -> Poll<io::Result<usize>> {
//         match Read::read(self.get_mut(), buf) {
//             Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
//             r => Poll::Ready(r)
//         }
//     }
// }

// struct AssertAsync<T>(T);

// impl<T: Read> Read for AssertAsync<T> {
//     fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
//         self.0.read(buf)
//     }
// }

// // impl<T: Read> AsyncRead for AssertAsync<T> {}

// #[cfg(feature = "tokio")]
// impl<T: AsyncRead + Read + Unpin> AsyncRead for AssertAsync<T> {
//     fn poll_read(
//         self: Pin<&mut Self>,
//         cx: &mut Context,
//         buf: &mut [u8],
//     ) -> Poll<io::Result<usize>> {
//         AsyncRead::poll_read(Pin::new(&mut self.get_mut().0), cx, buf)
//     }
// }

// struct AlwaysNotify<T>(T);

// impl<T: Future + Unpin> Future for AlwaysNotify<T> {
//     type Output = T::Output;

//     fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
//         let ret = Pin::new(&mut self.get_mut().0).poll(cx);
//         if let Poll::Pending = &ret {
//             println!("Notify");
//             cx.waker().wake_by_ref();
//         }
//         ret
//     }
// }

// #[test]
// fn test_gz_asyncread() {
//     println!("Starting runtime");
//     let mut rt = tokio::runtime::Runtime::new().unwrap();
//     let f = File::open("tests/good-file.gz").unwrap();

//     let mut content = Vec::new();

//     let mut gz_decoder = AssertAsync(GzDecoder::new(BadReader::new(f)));
//     let fut = async {
//         AsyncReadExt::read_to_end(&mut gz_decoder, &mut content).await.unwrap()
//     };
//     println!("Blocking on future");
//     rt.block_on(fut);

//     println!("Reading actual contents");
//     let mut expected = Vec::new();
//     File::open("tests/good-file.txt")
//         .unwrap()
//         .read_to_end(&mut expected)
//         .unwrap();

//     assert_eq!(content, expected);
// }

// #[test]
// fn test_multi_gz_asyncread() {
//     let mut rt = tokio::runtime::current_thread::Runtime::new().unwrap();
//     let f = File::open("tests/multi.gz").unwrap();

//     let mut content = Vec::new();

//     let mut multi_gz_decoder = AssertAsync(MultiGzDecoder::new(BadReader::new(f)));
//     let fut = async {
//         AsyncReadExt::read_to_end(&mut multi_gz_decoder, &mut content).await.unwrap()
//     };

//     rt.block_on(fut);

//     let mut expected = Vec::new();
//     File::open("tests/multi.txt")
//         .unwrap()
//         .read_to_end(&mut expected)
//         .unwrap();

//     assert_eq!(content, expected);
// }
