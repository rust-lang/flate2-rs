#![cfg(feature = "fdeflate")]

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};
use std::io::{Read, Write};

fn compress_raw(input: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(input.len() + 1024);
    let mut encoder = Compress::new(Compression::default(), false);
    let status = encoder
        .compress_vec(input, &mut encoded, FlushCompress::Finish)
        .unwrap();
    assert_eq!(status, Status::StreamEnd);
    encoded
}

fn decompress_raw(encoded: &[u8], expected_len: usize) -> Vec<u8> {
    let mut decoder = Decompress::new(false);
    let mut decoded = vec![0; expected_len];
    let status = decoder
        .decompress(encoded, &mut decoded, FlushDecompress::Finish)
        .unwrap();
    assert_eq!(status, Status::StreamEnd);
    decoded.truncate(decoder.total_out() as usize);
    decoded
}

#[test]
fn raw_roundtrip_with_one_byte_output() {
    let input = vec![b'a'; 128 * 1024];
    let encoded = compress_raw(&input);
    let mut decoder = Decompress::new(false);
    let mut decoded = Vec::with_capacity(input.len());
    let mut input_pos = 0;

    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let mut byte = [0; 1];
        let status = decoder
            .decompress(&encoded[input_pos..], &mut byte, FlushDecompress::None)
            .unwrap();
        input_pos += (decoder.total_in() - before_in) as usize;
        let written = (decoder.total_out() - before_out) as usize;
        decoded.extend_from_slice(&byte[..written]);

        if status == Status::StreamEnd {
            break;
        }

        assert!(
            written != 0 || decoder.total_in() != before_in,
            "decoder made no progress"
        );
    }

    assert_eq!(decoded, input);
    assert_eq!(input_pos, encoded.len());
}

#[test]
fn raw_trailing_bytes_remain_unconsumed() {
    let input = b"raw stream with extra data after it";
    let encoded = compress_raw(input);
    let mut with_trailing = encoded.clone();
    with_trailing.extend_from_slice(b"extra");

    let mut decoder = Decompress::new(false);
    let mut decoded = vec![0; 1024];
    let status = decoder
        .decompress(&with_trailing, &mut decoded, FlushDecompress::Finish)
        .unwrap();

    assert_eq!(status, Status::StreamEnd);
    assert_eq!(decoder.total_in(), encoded.len() as u64);
    assert_eq!(&decoded[..decoder.total_out() as usize], input);
}

#[test]
fn zlib_trailing_bytes_remain_unconsumed() {
    let input = b"zlib stream with extra data after it";
    let mut encoded = Vec::with_capacity(1024);
    let mut encoder = Compress::new(Compression::default(), true);
    assert_eq!(
        encoder
            .compress_vec(input, &mut encoded, FlushCompress::Finish)
            .unwrap(),
        Status::StreamEnd
    );
    let compressed_len = encoded.len();
    encoded.extend_from_slice(b"extra");

    let mut decoder = Decompress::new(true);
    let mut decoded = vec![0; 1024];
    let status = decoder
        .decompress(&encoded, &mut decoded, FlushDecompress::Finish)
        .unwrap();

    assert_eq!(status, Status::StreamEnd);
    assert_eq!(decoder.total_in(), compressed_len as u64);
    assert_eq!(&decoded[..decoder.total_out() as usize], input);
}

#[test]
fn partial_sync_and_full_flush_roundtrip() {
    for flush in [
        FlushCompress::Partial,
        FlushCompress::Sync,
        FlushCompress::Full,
    ] {
        let first = b"first half first half first half";
        let second = b"second half second half second half";
        let mut encoded = Vec::with_capacity(4096);
        let mut encoder = Compress::new(Compression::default(), false);
        encoder.compress_vec(first, &mut encoded, flush).unwrap();
        assert_eq!(
            encoder
                .compress_vec(second, &mut encoded, FlushCompress::Finish)
                .unwrap(),
            Status::StreamEnd
        );

        let mut expected = Vec::new();
        expected.extend_from_slice(first);
        expected.extend_from_slice(second);
        assert_eq!(decompress_raw(&encoded, expected.len()), expected);
    }
}

#[test]
fn read_and_write_wrappers_roundtrip_single_zero() {
    let input = [0];
    let direct_encoded = compress_raw(&input);
    assert_eq!(decompress_raw(&direct_encoded, input.len()), input);

    let mut encoder = flate2::read::DeflateEncoder::new(&input[..], Compression::default());
    let mut encoded = Vec::new();
    encoder.read_to_end(&mut encoded).unwrap();

    let mut reader = flate2::read::DeflateDecoder::new(&encoded[..]);
    let mut decoded = Vec::new();
    reader.read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, input);

    let mut writer = flate2::write::DeflateEncoder::new(
        flate2::write::DeflateDecoder::new(Vec::new()),
        Compression::default(),
    );
    writer.write_all(&input).unwrap();
    assert_eq!(writer.finish().unwrap().finish().unwrap(), input);
}
