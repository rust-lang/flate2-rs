//! Shared by deterministic regression tests and the differential fuzz target.
#![forbid(unsafe_code)]
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

pub fn exercise(data: &[u8]) {
    if data.len() < 8 {
        return;
    }
    let zlib = data[0] & 1 != 0;
    let level = data[1] % 10;
    let input = &data[8..];
    let mut compressor = Compress::new(Compression::new(level.into()), zlib);
    // Reset with both pending output and uncommitted parser work.
    compressor
        .compress(input, &mut [0; 7], FlushCompress::None)
        .unwrap();
    compressor.reset();
    let mut encoded = Vec::new();
    let mut output = vec![0; 32768];
    let chunk_size = 1 + u16::from_le_bytes([data[2], data[3]]) as usize;
    let flush = match data[4] % 4 {
        0 => FlushCompress::None,
        1 => FlushCompress::Partial,
        2 => FlushCompress::Sync,
        _ => FlushCompress::Full,
    };
    let out_size = 7 + u16::from_le_bytes([data[5], data[6]]) as usize % (output.len() - 7);
    let mut end = chunk_size.min(input.len());
    for iteration in 0..(input.len() + 100) * 10 {
        let before = (compressor.total_in(), compressor.total_out());
        let finishing = end == input.len();
        let mode = if finishing {
            FlushCompress::Finish
        } else {
            flush
        };
        // Zero-sized calls must neither lose input nor hang.
        let space = if iteration % 7 == 0 { 0 } else { out_size };
        let status = compressor
            .compress(&input[before.0 as usize..end], &mut output[..space], mode)
            .unwrap();
        let produced = (compressor.total_out() - before.1) as usize;
        assert!(produced <= space);
        assert!(compressor.total_in() as usize <= end);
        encoded.extend_from_slice(&output[..produced]);
        if status == Status::StreamEnd {
            break;
        }
        if !finishing && compressor.total_in() as usize == end && space > 0 && produced < space {
            end = (end + chunk_size).min(input.len());
        }
        if iteration + 1 == (input.len() + 100) * 10 {
            panic!("compression did not finish");
        }
    }
    assert_eq!(compressor.total_in() as usize, input.len());
    let independent = if zlib {
        miniz_oxide::inflate::decompress_to_vec_zlib(&encoded)
    } else {
        miniz_oxide::inflate::decompress_to_vec(&encoded)
    }
    .unwrap_or_else(|_| {
        panic!(
            "invalid encoding: controls={:?}, input_len={}, encoded={:?}",
            &data[..8],
            input.len(),
            &encoded[..encoded.len().min(100)]
        )
    });
    assert_eq!(independent, input);
    decode(&encoded, zlib, data, Some(input));
    // Corruption and truncation retain bounded progress. On successful decoding,
    // compare the result with an independent implementation.
    let mut malformed = encoded;
    if !malformed.is_empty() {
        let index = usize::from(data[7]) % malformed.len();
        malformed[index] ^= data[4];
        if data[0] & 2 != 0 {
            malformed.truncate(index);
        }
    }
    decode(&malformed, zlib, data, None);
    decode(input, zlib, data, None);
}

fn decode(encoded: &[u8], zlib: bool, controls: &[u8], expected: Option<&[u8]>) {
    let mut decoder = Decompress::new(zlib);
    let mut decoded = Vec::new();
    let mut output = [0; 8192];
    decoder
        .decompress(&[0xff; 10], &mut output, FlushDecompress::None)
        .ok();
    decoder.reset(zlib);
    let input_chunk = 1 + usize::from(controls[2]);
    let output_chunk = 1 + usize::from(controls[3]) * 31;
    let mut ended = false;
    for iteration in 0..(encoded.len() + 100) * 100 {
        let before = (decoder.total_in() as usize, decoder.total_out() as usize);
        let end = (before.0 + input_chunk).min(encoded.len());
        let space = if iteration % 11 == 0 { 0 } else { output_chunk };
        let result = decoder.decompress(
            &encoded[before.0..end],
            &mut output[..space],
            FlushDecompress::None,
        );
        let produced = decoder.total_out() as usize - before.1;
        assert!(produced <= space);
        assert!(decoder.total_in() as usize <= end);
        decoded.extend_from_slice(&output[..produced]);
        if matches!(result, Ok(Status::StreamEnd)) {
            ended = true;
            break;
        }
        if result.is_err() {
            break;
        }
        if space != 0 && before == (decoder.total_in() as usize, decoder.total_out() as usize) {
            assert!(expected.is_none(), "valid stream stalled");
            break;
        }
        // Do not let malformed streams turn a fuzz iteration into a zip bomb.
        if decoded.len() > 8 * 1024 * 1024 {
            return;
        }
    }
    if let Some(expected) = expected {
        assert!(ended);
        assert_eq!(decoded, expected);
        assert_eq!(decoder.total_in() as usize, encoded.len());
    } else if ended {
        let mut independent = vec![0; decoded.len() + 1];
        let mut state = miniz_oxide::inflate::stream::InflateState::new_boxed(if zlib {
            miniz_oxide::DataFormat::Zlib
        } else {
            miniz_oxide::DataFormat::Raw
        });
        let result = miniz_oxide::inflate::stream::inflate(
            &mut state,
            encoded,
            &mut independent,
            miniz_oxide::MZFlush::Finish,
        );
        assert_eq!(
            result.status,
            Ok(miniz_oxide::MZStatus::StreamEnd),
            "zlib={zlib}, encoded={encoded:?}, decoded_len={}",
            decoded.len()
        );
        assert_eq!(result.bytes_written, decoded.len());
        assert_eq!(&independent[..decoded.len()], decoded);
    }
}
