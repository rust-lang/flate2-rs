#![feature(async_await)]

extern crate flate2;
// extern crate tokio_io;
extern crate tokio;

use flate2::bufread::DeflateEncoder;
use flate2::Compression;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use tokio::io::{AsyncBufRead, BufReader};
use tokio::net::UnixStream;

// use tokio::io::{AsyncRead, AsyncWrite};

// Opens sample file, compresses the contents and returns a Vector
// fn open_hello_world() -> io::Result<Vec<u8>> {
//     let f = File::open("examples/hello_world.txt")?;
//     let b = BufReader::new(f);
//     let mut deflater = DeflateEncoder::new(b, Compression::fast());
//     let mut buffer = Vec::new();
//     deflater.read_to_end(&mut buffer)?;
//     Ok(buffer)
// }
// 
fn deflateencoder_read_hello_world() -> io::Result<Vec<u8>> {
   let mut ret_vec = [0;100];
   let c = b"hello world";
   let mut deflater = DeflateEncoder::new(&c[..], Compression::fast());
   let count = deflater.read(&mut ret_vec)?;
   Ok(ret_vec[0..count].to_vec())
}

fn async_deflate() {
    let bytes = b"Hello World!";

    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    let (mut server, client) = tokio::net::UnixStream::pair().expect("Could not build pair");

    let send_complete: () = async {
        let bytes = "{\"key1\":\"key with a paren set {}\",\"key2\":12345}{\"another\":\"part being sent\"}".as_bytes();

        server.write_all(bytes).await.expect("Failed to send");
        server.flush().await.expect("Failed to flush");

        // info!("Send complete");
    };


    // let send_compress = async {
    //     let mut deflater = DeflateEncoder::new(&b[..], Compresson::fast());
    // };
}
