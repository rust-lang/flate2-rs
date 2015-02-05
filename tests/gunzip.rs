#![feature(io, path)]

extern crate flate2;

use std::old_io::{IoErrorKind, IoResult, File};
use flate2::reader::GzDecoder;

// test extraction of a gzipped file
#[test]
fn test_extract_success() {
    let content = extract_file(&Path::new("tests/good-file.gz")).unwrap();
    let expected = File::open(&Path::new("tests/good-file.txt")).read_to_end().unwrap();
    assert!(content == expected);
}

// test extraction fails on a corrupt file
#[test]
fn test_extract_failure() {
    let result = extract_file(&Path::new("tests/corrupt-file.gz"));
    assert_eq!(result.err().unwrap().kind, IoErrorKind::InvalidInput);
}

// Tries to extract path into memory (assuming a .gz file).
fn extract_file(path_compressed: &Path) -> IoResult<Vec<u8>>{
    GzDecoder::new(File::open(path_compressed)).read_to_end()
}
