# flate2

A streaming compression/decompression library for rust with bindings to
[`miniz`](https://code.google.com/p/miniz/)

Supported formats:

* deflate
* zlib
* gzip

```toml
# Cargo.toml
[dependencies.flate2]
git = "https://github.com/alexcrichton/flate2-rs"
```

## Compression

```rust
extern crate flate2;

use std::io::MemWriter;
use flate2::{ZlibEncoder, Default};

fn main() {
    let mut e = ZlibEncoder::new(MemWriter::new(), Default);
    e.write(b"foo");
    e.write(b"bar");
    let compressed_bytes = e.finish();
}
```

## Decompression

```rust
extern crate flate2;

use std::io::BufReader;
use flate2::GzDecoder;

fn main() {
    let mut d = GzDecoder::new(BufReader::new(b"..."));
    println!("{}", d.read_to_str());
}
```
