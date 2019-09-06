pub mod deflate;
mod gz;
mod zio;
mod zlib;

use std::io;
use std::task::Poll;

pub fn cvt<T>(r: io::Result<T>) -> Poll<io::Result<T>> {
    match r {
        Ok(v) => Poll::Ready(Ok(v)),
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Poll::Pending,
        Err(e) => Poll::Ready(Err(e)),
    }
}

#[cfg(test)]
mod tests {
    use crate::tokio2::zio::AsyncWriter;
}
