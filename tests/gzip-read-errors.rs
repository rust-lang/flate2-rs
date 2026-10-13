use std::io::{self, BufReader, Read};

struct FailReads<'a> {
    data: &'a [u8],
    remaining: usize,
    kind: io::ErrorKind,
}

impl Read for FailReads<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining > 0 {
            self.remaining -= 1;
            return Err(self.kind.into());
        }
        self.data.read(buf)
    }
}

fn encoder(source: FailReads<'_>, buffered: bool) -> Box<dyn Read + '_> {
    if buffered {
        Box::new(flate2::bufread::GzEncoder::new(
            BufReader::new(source),
            flate2::Compression::default(),
        ))
    } else {
        Box::new(flate2::read::GzEncoder::new(
            source,
            flate2::Compression::default(),
        ))
    }
}

fn assert_roundtrip(compressed: &[u8], input: &[u8]) {
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(compressed)
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn gzip_header_survives_interrupted_input() {
    let input = b"a signal should not corrupt the gzip header";
    for buffered in [false, true] {
        let source = FailReads {
            data: input,
            remaining: 1,
            kind: io::ErrorKind::Interrupted,
        };
        let mut compressed = Vec::new();
        encoder(source, buffered)
            .read_to_end(&mut compressed)
            .unwrap();
        assert_roundtrip(&compressed, input);
    }
}

#[test]
fn gzip_reports_header_progress_before_input_error() {
    let input = b"retrying a nonblocking source must preserve the header";
    for buffered in [false, true] {
        let source = FailReads {
            data: input,
            remaining: 2,
            kind: io::ErrorKind::WouldBlock,
        };
        let mut encoder = encoder(source, buffered);
        let mut buffer = [0; 64];
        let count = encoder.read(&mut buffer).unwrap();
        assert_eq!(count, 10);
        let mut compressed = buffer[..count].to_vec();
        assert_eq!(
            encoder.read(&mut buffer).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        encoder.read_to_end(&mut compressed).unwrap();
        assert_roundtrip(&compressed, input);
    }
}
