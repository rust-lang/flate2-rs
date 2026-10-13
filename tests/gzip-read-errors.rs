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
            return Err(io::Error::new(self.kind, "injected input error"));
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
fn gzip_reports_one_shot_input_errors() {
    let input = b"a one-shot error must not disappear after the gzip header";
    for kind in [
        io::ErrorKind::WouldBlock,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::Other,
    ] {
        for buffered in [false, true] {
            let source = FailReads {
                data: input,
                remaining: 1,
                kind,
            };
            let mut encoder = encoder(source, buffered);
            let mut compressed = Vec::new();
            let err = encoder.read_to_end(&mut compressed).unwrap_err();
            assert_eq!(err.kind(), kind);
            assert_eq!(err.to_string(), "injected input error");
            assert_eq!(compressed.len(), 10);

            encoder.read_to_end(&mut compressed).unwrap();
            assert_roundtrip(&compressed, input);
        }
    }
}

#[test]
fn gzip_reports_header_progress_before_input_error() {
    let input = b"retrying a nonblocking source must preserve the header";
    for kind in [io::ErrorKind::Interrupted, io::ErrorKind::WouldBlock] {
        for remaining in [1, 2] {
            for buffered in [false, true] {
                let source = FailReads {
                    data: input,
                    remaining,
                    kind,
                };
                let mut encoder = encoder(source, buffered);
                let mut buffer = [0; 64];
                assert_eq!(encoder.read(&mut []).unwrap(), 0);
                let count = encoder.read(&mut buffer).unwrap();
                assert_eq!(count, 10);
                let mut compressed = buffer[..count].to_vec();
                for _ in 0..remaining {
                    assert_eq!(encoder.read(&mut []).unwrap(), 0);
                    assert_eq!(encoder.read(&mut []).unwrap(), 0);
                    let err = encoder.read(&mut buffer).unwrap_err();
                    assert_eq!(err.kind(), kind);
                    assert_eq!(err.to_string(), "injected input error");
                }
                encoder.read_to_end(&mut compressed).unwrap();
                assert_roundtrip(&compressed, input);
            }
        }
    }
}

#[test]
fn gzip_reads_header_and_body_together() {
    for input in [
        &b""[..],
        &b"read the header and compressed body together"[..],
    ] {
        for buffered in [false, true] {
            for header_prefix in [0, 7, 10] {
                let source = FailReads {
                    data: input,
                    remaining: 0,
                    kind: io::ErrorKind::Other,
                };
                let mut encoder = encoder(source, buffered);
                let mut compressed = vec![0; header_prefix];
                encoder.read_exact(&mut compressed).unwrap();
                let mut buffer = [0; 64];
                let count = encoder.read(&mut buffer).unwrap();
                assert!(count > 10 - header_prefix);
                compressed.extend_from_slice(&buffer[..count]);
                encoder.read_to_end(&mut compressed).unwrap();
                assert_roundtrip(&compressed, input);
            }
        }
    }
}
