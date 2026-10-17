//! Streaming contracts shared by all backends.
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

fn random(len: usize) -> Vec<u8> {
    let mut state = 0x12345678u32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

fn encode(input: &[u8], zlib: bool) -> Vec<u8> {
    let mut encoder = Compress::new(Compression::default(), zlib);
    let mut output = Vec::with_capacity(input.len() * 2 + 1024);
    assert_eq!(
        encoder
            .compress_vec(input, &mut output, FlushCompress::Finish)
            .unwrap(),
        Status::StreamEnd
    );
    assert_eq!(encoder.total_in() as usize, input.len());
    output
}

#[test]
fn empty_stream_and_separate_trailer_need_no_spare_output() {
    for zlib in [false, true] {
        let encoded = encode(b"", zlib);
        let mut decoder = Decompress::new(zlib);
        assert_eq!(
            decoder
                .decompress(&encoded, &mut [], FlushDecompress::Finish)
                .unwrap(),
            Status::StreamEnd
        );
        assert_eq!(decoder.total_in() as usize, encoded.len());
    }
    let encoded = encode(b"hello", true);
    let split = encoded.len() - 4;
    let mut decoder = Decompress::new(true);
    let mut output = [0; 5];
    decoder
        .decompress(&encoded[..split], &mut output, FlushDecompress::None)
        .unwrap();
    assert_eq!(&output, b"hello");
    assert_eq!(
        decoder
            .decompress(
                &encoded[decoder.total_in() as usize..],
                &mut [],
                FlushDecompress::Finish
            )
            .unwrap(),
        Status::StreamEnd
    );
    assert_eq!(decoder.total_in() as usize, encoded.len());
}

#[test]
fn large_finish_and_vec_output_complete_in_one_call() {
    let input = random(1024 * 1024);
    for zlib in [false, true] {
        let encoded = encode(&input, zlib);
        let mut decoder = Decompress::new(zlib);
        let mut output = vec![0; input.len()];
        assert_eq!(
            decoder
                .decompress(&encoded, &mut output, FlushDecompress::Finish)
                .unwrap(),
            Status::StreamEnd
        );
        assert_eq!(output, input);
        decoder.reset(zlib);
        let mut output = Vec::with_capacity(input.len());
        assert_eq!(
            decoder
                .decompress_vec(&encoded, &mut output, FlushDecompress::Finish)
                .unwrap(),
            Status::StreamEnd
        );
        assert_eq!(output, input);
    }
}

#[test]
fn decoding_chunk_schedules_preserve_history_and_trailing_bytes() {
    for zlib in [false, true] {
        for size in [1, 32768, 65536, 98305, 270000] {
            let mut input = random(size);
            for i in 32768..size {
                input[i] = input[i - 32768];
            }
            let mut encoded = encode(&input, zlib);
            let end = encoded.len();
            encoded.extend_from_slice(b"trailing bytes");
            for (ic, oc) in [
                (1, 1),
                (7, 1),
                (1, 4096),
                (7, 997),
                (4096, 32768),
                (4096, 131072),
                (7, 131072),
                (999999, 65536),
                (999999, 999999),
            ] {
                let mut decoder = Decompress::new(zlib);
                let mut output = vec![0; size];
                loop {
                    let (bi, bo) = (decoder.total_in() as usize, decoder.total_out() as usize);
                    let status = decoder
                        .decompress(
                            &encoded[bi..(bi + ic).min(encoded.len())],
                            &mut output[bo..(bo + oc).min(size)],
                            FlushDecompress::None,
                        )
                        .unwrap();
                    assert!(decoder.total_in() as usize <= end);
                    if status == Status::StreamEnd {
                        break;
                    }
                    assert_ne!(
                        (bi as u64, bo as u64),
                        (decoder.total_in(), decoder.total_out()),
                        "size={size} chunks={ic},{oc} zlib={zlib}"
                    );
                }
                assert_eq!(decoder.total_in() as usize, end);
                assert_eq!(output, input);
            }
        }
    }
}

#[test]
fn alternating_slice_and_vec_output_preserves_decoder_history() {
    let mut input = random(500000);
    for i in 32768..input.len() {
        input[i] = input[i - 32768];
    }
    for zlib in [false, true] {
        let encoded = encode(&input, zlib);
        let mut decoder = Decompress::new(zlib);
        let mut decoded = Vec::new();
        let mut slice = vec![0; 131072];
        let mut spare = Vec::with_capacity(131072);
        let mut step = 0;
        loop {
            let (bi, bo) = (decoder.total_in() as usize, decoder.total_out());
            let end = (bi + 4096).min(encoded.len());
            let status = if step % 2 == 0 {
                let status = decoder
                    .decompress(&encoded[bi..end], &mut slice, FlushDecompress::None)
                    .unwrap();
                decoded.extend_from_slice(&slice[..(decoder.total_out() - bo) as usize]);
                status
            } else {
                spare.clear();
                let status = decoder
                    .decompress_vec(&encoded[bi..end], &mut spare, FlushDecompress::None)
                    .unwrap();
                decoded.extend_from_slice(&spare);
                status
            };
            if status == Status::StreamEnd {
                break;
            }
            assert_ne!((bi as u64, bo), (decoder.total_in(), decoder.total_out()));
            step += 1;
        }
        assert_eq!(decoded, input);
        assert_eq!(decoder.total_in() as usize, encoded.len());
    }
}
