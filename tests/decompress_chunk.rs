#[test]
fn deflate_decoder_partial() {
    // Decompresses to
    // "* QUOTAROOT INBOX \"User quota\"\r\n* QUOTA \"User quota\" (STORAGE 76 307200)\r\nA0001 OK Getquotaroot completed (0.001 + 0.000 secs).\r\n"
    let input = vec![
        210, 82, 8, 12, 245, 15, 113, 12, 242, 247, 15, 81, 240, 244, 115, 242, 143, 80, 80, 10,
        45, 78, 45, 82, 40, 44, 205, 47, 73, 84, 226, 229, 210, 130, 200, 163, 136, 42, 104, 4,
        135, 248, 7, 57, 186, 187, 42, 152, 155, 41, 24, 27, 152, 27, 25, 24, 104, 242, 114, 57,
        26, 24, 24, 24, 42, 248, 123, 43, 184, 167, 150, 128, 213, 21, 229, 231, 151, 40, 36, 231,
        231, 22, 228, 164, 150, 164, 166, 40, 104, 24, 232, 129, 20, 104, 43, 128, 104, 3, 133,
        226, 212, 228, 98, 77, 61, 94, 46, 0, 0, 0, 0, 255, 255,
    ];

    // Create very small output buffer.
    let mut output = vec![0; 8];

    let zlib_header = false;
    let mut decompress = flate2::Decompress::new(zlib_header);

    let flush_decompress = flate2::FlushDecompress::None;
    let status = decompress
        .decompress(&input, &mut output, flush_decompress)
        .unwrap();
    assert_eq!(status, flate2::Status::Ok);

    // Should not consume everything, there is not enough space in the buffer for the output.
    assert_ne!(decompress.total_in(), input.len() as u64);
}
