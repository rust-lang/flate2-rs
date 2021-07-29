extern crate flate2;

use flate2::read::GzDecoder;
use flate2::read::MultiGzDecoder;
use std::fs::File;
use std::io::prelude::*;
use std::io::{self, BufReader, BufWriter};
use std::path::Path;

fn hex_to_bin(hex_reader: impl Read, bin_writer: impl Write) {
    let mut bin_writer = BufWriter::new(bin_writer);
    let hex_reader = BufReader::new(hex_reader);

    let mut hex = String::new();
    let mut bytes = hex_reader.bytes();
    loop {
        let d = if let Some(b) = bytes.next() {
            String::from_utf8(vec![b.unwrap()]).unwrap()
        } else {
            "".into()
        };
        if !hex.is_empty() && (&d == " " || d.is_empty()) {
            bin_writer
                .write_all(&[u8::from_str_radix(&hex, 16).unwrap()])
                .unwrap();
            hex.clear();
        } else if &d != " " {
            hex.push_str(&d);
        }
        if hex.is_empty() && d.is_empty() {
            break;
        }
    }

    bin_writer.flush().unwrap();
}

// test extraction of a gzipped file
#[test]
fn test_extract_success() {
    let content = extract_file(Path::new("tests/good-file.gz")).unwrap();
    let mut expected = Vec::new();
    File::open("tests/good-file.txt")
        .unwrap()
        .read_to_end(&mut expected)
        .unwrap();
    assert!(content == expected);
}
//
// test partial extraction of a multistream gzipped file
#[test]
fn test_extract_success_partial_multi() {
    let content = extract_file(Path::new("tests/multi.gz")).unwrap();
    let mut expected = String::new();
    BufReader::new(File::open("tests/multi.txt").unwrap())
        .read_line(&mut expected)
        .unwrap();
    assert_eq!(content, expected.as_bytes());
}

// test extraction fails on a corrupt file
#[test]
fn test_extract_failure() {
    let result = extract_hexfile(Path::new("tests/corrupt-file.gz.hex"));
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::InvalidInput);
}

//test complete extraction of a multistream gzipped file
#[test]
fn test_extract_success_multi() {
    let content = extract_file_multi(Path::new("tests/multi.gz")).unwrap();
    let mut expected = Vec::new();
    File::open("tests/multi.txt")
        .unwrap()
        .read_to_end(&mut expected)
        .unwrap();
    assert_eq!(content, expected);
}

// Tries to extract path into memory (assuming a .gz file).
fn extract_file(path_compressed: &Path) -> io::Result<Vec<u8>> {
    let mut v = Vec::new();
    let f = File::open(path_compressed)?;
    GzDecoder::new(f).read_to_end(&mut v)?;
    Ok(v)
}
// Tries to extract path into memory (assuming a .gz hex file).
fn extract_hexfile(path_compressed: &Path) -> io::Result<Vec<u8>> {
    let mut v = Vec::new();
    let mut bin = Vec::new();
    hex_to_bin(File::open(path_compressed).unwrap(), &mut bin);
    GzDecoder::new(bin.as_slice()).read_to_end(&mut v)?;
    Ok(v)
}
// Tries to extract path into memory (decompressing all members in case
// of a multi member .gz file).
fn extract_file_multi(path_compressed: &Path) -> io::Result<Vec<u8>> {
    let mut v = Vec::new();
    let f = File::open(path_compressed)?;
    MultiGzDecoder::new(f).read_to_end(&mut v)?;
    Ok(v)
}

#[test]
fn empty_error_once() {
    let data: &[u8] = &[];
    let cbjson = GzDecoder::new(data);
    let reader = BufReader::new(cbjson);
    let mut stream = reader.lines();
    assert!(stream.next().unwrap().is_err());
    assert!(stream.next().is_none());
}
