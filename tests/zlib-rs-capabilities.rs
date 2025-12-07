//! Validate that certain feature-gated functionality is still available.
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};

#[test]
fn test_compress_new_with_window_bits() {
    let string = "hello world".as_bytes();

    // Test with window_bits = 9 (minimum)
    let mut encoded = Vec::with_capacity(1024);
    let mut encoder = Compress::new_with_window_bits(Compression::default(), true, 9);
    encoder
        .compress_vec(string, &mut encoded, FlushCompress::Finish)
        .unwrap();
    assert_ne!(encoded.len(), 0);

    let mut decoder = Decompress::new_with_window_bits(true, 9);
    let mut decoded = [0; 1024];
    decoder
        .decompress(&encoded, &mut decoded, FlushDecompress::Finish)
        .unwrap();
    assert_eq!(&decoded[..string.len()], string);

    // Test with window_bits = 15 (maximum)
    let mut encoded = Vec::with_capacity(1024);
    let mut encoder = Compress::new_with_window_bits(Compression::default(), false, 15);
    encoder
        .compress_vec(string, &mut encoded, FlushCompress::Finish)
        .unwrap();
    assert_ne!(encoded.len(), 0);

    let mut decoder = Decompress::new_with_window_bits(false, 15);
    let mut decoded = [0; 1024];
    decoder
        .decompress(&encoded, &mut decoded, FlushDecompress::Finish)
        .unwrap();
    assert_eq!(&decoded[..string.len()], string);
}

#[test]
fn test_decompress_new_gzip_window_bits() {
    let string = "hello world".as_bytes();

    // Test with different window_bits values
    for window_bits in [9, 12, 15] {
        let mut encoded = Vec::with_capacity(1024);
        let mut encoder = Compress::new_gzip(Compression::default(), window_bits);
        encoder
            .compress_vec(string, &mut encoded, FlushCompress::Finish)
            .unwrap();

        let mut decoder = Decompress::new_gzip(window_bits);
        let mut decoded = [0; 1024];
        decoder
            .decompress(&encoded, &mut decoded, FlushDecompress::Finish)
            .unwrap();
        assert_eq!(
            &decoded[..string.len()],
            string,
            "Failed with window_bits={}",
            window_bits
        );
    }
}

#[test]
#[should_panic(expected = "window_bits must be within 9 ..= 15")]
fn test_compress_new_with_window_bits_invalid_low() {
    let _ = Compress::new_with_window_bits(Compression::default(), true, 8);
}

#[test]
#[should_panic(expected = "window_bits must be within 9 ..= 15")]
fn test_compress_new_with_window_bits_invalid_high() {
    let _ = Compress::new_with_window_bits(Compression::default(), true, 16);
}

#[test]
#[should_panic(expected = "window_bits must be within 9 ..= 15")]
fn test_compress_new_gzip_invalid_low() {
    let _ = Compress::new_gzip(Compression::default(), 8);
}

#[test]
#[should_panic(expected = "window_bits must be within 9 ..= 15")]
fn test_compress_new_gzip_invalid_high() {
    let _ = Compress::new_gzip(Compression::default(), 16);
}
