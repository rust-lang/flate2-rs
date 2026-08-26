use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::io::{self, Read, Write};

/// Accepts at most seven bytes per write and returns one `Interrupted` error
/// after writing at least 1 KiB.
#[derive(Default)]
struct PartialWriter {
    output: Vec<u8>,
    interrupted: bool,
}

impl Write for PartialWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.interrupted && self.output.len() >= 1024 {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }

        let len = buf.len().min(7);
        self.output.extend_from_slice(&buf[..len]);
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn encoder_handles_partial_writes() {
    let input: Vec<u8> = StdRng::seed_from_u64(0x1234_5678_9abc_def0)
        .random_iter()
        .take(128 * 1024)
        .collect();

    let mut expected = DeflateEncoder::new(Vec::new(), Compression::fast());
    expected.write_all(&input).unwrap();
    expected.flush().unwrap();
    let expected = expected.finish().unwrap();

    let mut encoder = DeflateEncoder::new(PartialWriter::default(), Compression::fast());
    encoder.write_all(&input).unwrap();
    loop {
        match encoder.flush() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result.unwrap(),
        }
        break;
    }
    let writer = encoder.finish().unwrap();
    assert!(
        writer.interrupted,
        "the downstream writer was never interrupted, but it should interrupt after 1 KiB"
    );
    assert_eq!(
        writer.output, expected,
        "partial writes changed the compressed output"
    );

    let mut decoded = Vec::new();
    DeflateDecoder::new(writer.output.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, input, "decoded output differs from the input");
}
