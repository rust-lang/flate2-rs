extern crate flate2;

use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::path::Path;
use flate2::read::GzDecoder;

// test extraction of a gzipped file
#[test]
fn test_extract_success() {
    let content = extract_file(Path::new("tests/good-file.gz")).unwrap();
    let mut expected = Vec::new();
    File::open("tests/good-file.txt").unwrap().read_to_end(&mut expected).unwrap();
    assert!(content == expected);
}

// test extraction fails on a corrupt file
#[test]
fn test_extract_failure() {
    let result = extract_file(Path::new("tests/corrupt-file.gz"));
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::InvalidInput);
}

// Tries to extract path into memory (assuming a .gz file).
fn extract_file(path_compressed: &Path) -> io::Result<Vec<u8>>{
    let mut v = Vec::new();
    let f = try!(File::open(path_compressed));
    try!(try!(GzDecoder::new(f)).read_to_end(&mut v));
    Ok(v)
}
