# flate2

[![Crates.io](https://img.shields.io/crates/v/flate2.svg?maxAge=2592000)](https://crates.io/crates/flate2)
[![Documentation](https://docs.rs/flate2/badge.svg)](https://docs.rs/flate2)

A streaming compression/decompression library DEFLATE-based streams in Rust.

This crate by default implemented as a wrapper around the `miniz_oxide` crate, a
port of `miniz.c` to Rust. This crate can also optionally use other [backends](#Backends) like the zlib library
or `miniz.c` itself.

Supported formats:

* deflate
* zlib
* gzip

```toml
# Cargo.toml
[dependencies]
flate2 = "1.0"
```

## Compression

```rust
use std::io::prelude::*;
use flate2::Compression;
use flate2::write::ZlibEncoder;

fn main() {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(b"foo");
    e.write_all(b"bar");
    let compressed_bytes = e.finish();
}
```

## Decompression

```rust,no_run
use std::io::prelude::*;
use flate2::read::GzDecoder;

fn main() {
    let mut d = GzDecoder::new("...".as_bytes());
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    println!("{}", s);
}
```

## Backends

Using zlib instead of the (default) Rust backend:

```toml
[dependencies]
flate2 = { version = "1.0", features = ["zlib"], default-features = false }
```

The cloudflare optimized version of zlib is also available.
While it's significantly faster it requires a x86-64 CPU with SSE 4.2 or ARM64 with NEON & CRC.
It does not support 32-bit CPUs at all and is incompatible with mingw.
For more information check the [crate documentation](https://crates.io/crates/cloudflare-zlib-sys).

```toml
[dependencies]
flate2 = { version = "1.0", features = ["cloudflare_zlib"], default-features = false }
```

Using `miniz.c`:

```toml
[dependencies]
flate2 = { version = "1.0", features = ["miniz-sys"], default-features = false }
```

# License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
