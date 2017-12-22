extern crate flate2;

use std::fs::File;
use std::io::prelude::*;
use std::io::{self, BufReader};
use std::path::Path;
use flate2::read::GzDecoder;
use flate2::read::MultiGzDecoder;

// test extraction of a gzipped file
#[test]
fn test_extract_success() {
    let content = extract_file(Path::new("tests/good-file.gz")).unwrap();
    let mut expected = Vec::new();
    File::open("tests/good-file.txt").unwrap().read_to_end(&mut expected).unwrap();
    assert!(content == expected);
}
//
// test partial extraction of a multistream gzipped file
#[test]
fn test_extract_success_partial_multi() {
    let content = extract_file(Path::new("tests/multi.gz")).unwrap();
    let mut expected = String::new();
    BufReader::new(File::open("tests/multi.txt").unwrap()).read_line(&mut expected).unwrap();
    assert_eq!(content, expected.as_bytes());
}

// test extraction fails on a corrupt file
#[test]
fn test_extract_failure() {
    let result = extract_file(Path::new("tests/corrupt-file.gz"));
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::InvalidInput);
}

//test complete extraction of a multistream gzipped file
#[test]
fn test_extract_success_multi() {
    let content = extract_file_multi(Path::new("tests/multi.gz")).unwrap();
    let mut expected = Vec::new();
    File::open("tests/multi.txt").unwrap().read_to_end(&mut expected).unwrap();
    assert_eq!(content, expected);
}

// Tries to extract path into memory (assuming a .gz file).
fn extract_file(path_compressed: &Path) -> io::Result<Vec<u8>>{
    let mut v = Vec::new();
    let f = try!(File::open(path_compressed));
    try!(GzDecoder::new(f).read_to_end(&mut v));
    Ok(v)
}

// Tries to extract path into memory (decompressing all members in case
// of a multi member .gz file).
fn extract_file_multi(path_compressed: &Path) -> io::Result<Vec<u8>>{
    let mut v = Vec::new();
    let f = try!(File::open(path_compressed));
    try!(MultiGzDecoder::new(f).read_to_end(&mut v));
    Ok(v)
}

// Test writing and reading a gz stream via std::io::Write
#[test]
fn test_gz_write() {
    let buf = Vec::new();
    let mut w = flate2::write::GzEncoder::new(buf, flate2::Compression::new(0));
    let content = b"one two three four five";
    w.write_all(content).expect("could not write to gz");
    let gz = w.finish().unwrap();
    let buf = Vec::new();
    let mut w = flate2::write::GzDecoder::new(buf);
    w.write_all(&gz).expect("could write to gz decoder");
    let out = w.finish().unwrap();
    assert_eq!(content, &out[..]);
}
