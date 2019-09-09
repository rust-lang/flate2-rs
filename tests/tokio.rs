#![cfg(feature = "tokio")]

extern crate flate2;
extern crate rand;
extern crate tokio;
// extern crate tokio_io;
// extern crate tokio_tcp;
// extern crate tokio_threadpool;

use std::io::{Read, Write};
use std::iter;
// use std::net::{Shutdown, TcpListener};
use std::thread;

use flate2::read;
use flate2::write;
use flate2::Compression;
// use futures::Future;
use rand::{thread_rng, Rng};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufWriter, AsyncWrite, BufReader};
// use tokio::ioio::{copy, shutdown};
// use tokio_tcp::TcpStream;
use tokio::net::{TcpStream, TcpListener};

async fn connect_and_send(addr: std::net::SocketAddr, data: Vec<u8>) {
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut writer = write::tokio2::DeflateEncoder::new(stream, Compression::default());
    
    writer.write_all(&data).await.unwrap();
    // writer.flush().await.unwrap();
    writer.shutdown().await.unwrap();
}

#[test]
fn deflate_async_compress_decompress() {
    let test_data = iter::repeat(())
        .take(1024 * 1024)
        .map(|()| thread_rng().gen::<u8>())
        .collect::<Vec<_>>();

    let test_data = test_data.as_slice();
    
    // let test_data = b"Hello, World!";
    
    let write = async move {
        let mut data: Vec<u8> = Vec::new();
        let mut writer = write::tokio2::DeflateEncoder::new(&mut data, Compression::default());
        writer.write_all(test_data).await.unwrap();
        writer.shutdown().await.unwrap();

        data
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let async_compressed_data = rt.block_on(write);

    let mut sync_writer = write::DeflateEncoder::new(Vec::new(), Compression::default());
    sync_writer.write_all(test_data).unwrap();
    let sync_compressed_data = sync_writer.finish().unwrap();

    assert_eq!(async_compressed_data, sync_compressed_data);

    let read_async = async {
        let mut data = Vec::new();
        let mut reader = read::tokio2::DeflateDecoder::new(async_compressed_data.as_slice());
        reader.read_to_end(&mut data).await.unwrap();
        
        data
    };

    let mut sync_decompressed_data: Vec<u8> = Vec::new();
    let mut sync_reader = read::DeflateDecoder::new(sync_compressed_data.as_slice());
    std::io::Read::read_to_end(&mut sync_reader, &mut sync_decompressed_data).unwrap();

    let async_decompressed_data = rt.block_on(read_async);

    assert_eq!(async_decompressed_data, sync_decompressed_data);
    assert_eq!(async_decompressed_data, test_data);
}

// #[test]
fn test_async_read_write() {
    let random_data = iter::repeat(())
        .take(1024 * 1024)
        .map(|()| thread_rng().gen::<u8>())
        .collect::<Vec<_>>();

    let rt = tokio::runtime::Runtime::new().unwrap();

    let create_stream = async {
        let stream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        stream
    };

    let read_data = |client| async {
        let mut reader = read::tokio2::DeflateDecoder::new(client);

        let mut read_data = Vec::new();
        reader.read_to_end(&mut read_data).await.unwrap();

        read_data
    };

    let test = async move {
        let mut listener: TcpListener = create_stream.await;
        let addr = listener.local_addr().unwrap();
        
        tokio::spawn(connect_and_send(addr, random_data.clone()));

        let (client, _) = listener.accept().await.unwrap();
        let read_data = read_data(client).await;

        for (l, r) in random_data.iter().zip(read_data.iter()) {
            if l != r {
                panic!();
            }
        }

        println!("{}, {}", random_data.len(), read_data.len());

        assert_eq!(random_data, read_data);
    };
    
    rt.block_on(test);
}

// #[test]
// fn tcp_stream_echo_pattern() {
//     const N: u8 = 16;
//     const M: usize = 16 * 1024;

//     let listener = TcpListener::bind("127.0.0.1:0").unwrap();
//     let addr = listener.local_addr().unwrap();
//     let t = thread::spawn(move || {
//         let mut a = listener.accept().unwrap().0;
//         let mut b = a.try_clone().unwrap();

//         let t = thread::spawn(move || {
//             // let mut b = read::DeflateDecoder::new(b);
//             let mut buf = [0; M];
//             for i in 0..N {
//                 b.read_exact(&mut buf).unwrap();
//                 for byte in buf.iter() {
//                     assert_eq!(*byte, i);
//                 }
//             }

//             assert_eq!(b.read(&mut buf).unwrap(), 0);
//         });

//         // let mut a = write::ZlibEncoder::new(a, Compression::default());
//         for i in 0..N {
//             let buf = [i; M];
//             a.write_all(&buf).unwrap();
//         }
//         a.shutdown(Shutdown::Write).unwrap();

//         t.join().unwrap();
//     });

    
//     let copy = async move {
//         let stream = TcpStream::connect(&addr).await.unwrap();
//         let (mut a, mut b) = stream.split();
//         // let a = read::ZlibDecoder::new(a);
//         // let mut b = write::DeflateEncoder::new(b, Compression::default());
        
//         let result = a.copy(&mut b).await;
        
//         let amt= result.unwrap();
//         assert_eq!(amt, (N as u64) * (M as u64));
//         b.shutdown().await.unwrap();
//     };
        
//     let rt = tokio::runtime::Runtime::new().unwrap();
//     rt.spawn(copy);
//     rt.shutdown_on_idle();
//     t.join().unwrap();
// }

// #[test]
// fn echo_random() {
//     let v = iter::repeat(())
//         .take(1024 * 1024)
//         .map(|()| thread_rng().gen::<u8>())
//         .collect::<Vec<_>>();

//     let listener = TcpListener::bind("127.0.0.1:0").unwrap();
//     let addr = listener.local_addr().unwrap();
//     let v2 = v.clone();
//     let t = thread::spawn(move || {
//         let mut a = listener.accept().unwrap().0;
//         let mut b = a.try_clone().unwrap();

//         let mut v3 = v2.clone();
//         let t = thread::spawn(move || {
//             // let mut b = read::DeflateDecoder::new(b);
//             let mut buf = [0; 1024];
//             while v3.len() > 0 {
//                 let n = b.read(&mut buf).unwrap();
//                 for (actual, expected) in buf[..n].iter().zip(&v3) {
//                     assert_eq!(*actual, *expected);
//                 }
//                 v3.drain(..n);
//             }

//             assert_eq!(b.read(&mut buf).unwrap(), 0);
//         });

//         // let mut a = write::ZlibEncoder::new(a, Compression::default());
//         a.write_all(&v2).unwrap();
//         a.shutdown(Shutdown::Write).unwrap();

//         t.join().unwrap();
//     });

//     let copy = async move {
//             let stream = TcpStream::connect(&addr).await.unwrap();
//             let (mut a, mut b) = stream.split();
//             // let a = read::ZlibDecoder::new(a);
//             // let b = write::DeflateEncoder::new(b, Compression::default());
//             let amt = a.copy(&mut b).await.unwrap();
    
//             assert_eq!(amt, v.len() as u64);
//             b.shutdown().await.unwrap();
//     };

//     let rt = tokio::runtime::Runtime::new().unwrap();
//     rt.spawn(copy);
//     rt.shutdown_on_idle();
//     t.join().unwrap();
// }
