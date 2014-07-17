# flate2

A streaming compression/decompression library for rust with bindings to
[`miniz`](https://code.google.com/p/miniz/)

Supported formats:

* zlib
* gzip

```toml
# Cargo.toml
[dependencies.flate2]
git = "https://github.com/alexcrichton/flate2-rs"
```

## zlib Compression

```rust
extern crate flate2;

use std::io::MemWriter;
use flate2::{Encoder, Default};

fn main() {
    let mut e = Encoder::new(MemWriter::new(), Default);
    e.write(b"foo");
    e.write(b"bar");
    let compressed_bytes = e.finish();
}
```

## zlib Decompression

```rust
extern crate flate2;

use std::io::BufReader;
use flate2::Decoder;

fn main() {
    let mut d = Decoder::new(BufReader::new(b"..."));
    println!("{}", d.read_to_str());
}
```

## gzip Compression

```rust
extern crate flate2;

use std::io::MemWriter;
use flate2::Default;
use flate2::gz::Encoder;

fn main() {
    let mut e = Encoder::new(MemWriter::new(), Default);
    e.write(b"foo");
    e.write(b"bar");
    let compressed_bytes = e.finish();
}
```

## gzip Decompression

```rust
extern crate flate2;

use std::io::BufReader;
use flate2::gz::Decoder;

fn main() {
    let mut d = Decoder::new(BufReader::new(b"..."));
    println!("{}", d.read_to_str());
}
```
