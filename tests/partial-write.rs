use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::{self, Read, Write};

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
    let mut state = 0x1234_5678_9abc_def0u64;
    let input: Vec<_> = (0..128 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
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
    assert!(writer.interrupted);
    assert_eq!(writer.output, expected);

    let mut decoded = Vec::new();
    DeflateDecoder::new(writer.output.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, input);
}
