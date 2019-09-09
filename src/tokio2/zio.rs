use std::io;

#[cfg(feature = "tokio")]
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::ready;
use pin_project::{pin_project, project};

#[cfg(feature = "tokio")]
use tokio::io::{AsyncWrite, BufWriter};

use crate::zio::{Flush, Ops};
use crate::Status;

#[pin_project]
#[derive(Debug)]
pub struct AsyncWriter<W: AsyncWrite, D: Ops> {
    #[pin]
    obj: BufWriter<W>,
    pub data: D,
    buf: Vec<u8>,
}

impl<W: AsyncWrite, D: Ops> AsyncWriter<W, D> {
    pub fn new(w: W, d: D) -> AsyncWriter<W, D> {
        AsyncWriter {
            obj: BufWriter::with_capacity(crate::DEFAULT_CAPACITY, w),
            data: d,
            buf: Vec::with_capacity(crate::DEFAULT_CAPACITY),
        }
    }

    pub fn finish(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        loop {
            ready!(self.as_mut().dump(cx))?;
            let mut this = self.project();

            let before = this.data.total_out();
            this.data.run_vec(&[], &mut this.buf, D::Flush::finish())?;

            if before == self.data.total_out() {
                return Poll::Ready(Ok(()));
            }
        }
    }

    // pub fn replace(&mut self, w: W) -> W {
    //     self.buf.truncate(0);
    //     mem::replace(self.get_mut(), w)
    // }

    pub fn get_ref(&self) -> &W {
        &self.obj.get_ref()
    }

    pub fn get_mut(&mut self) -> &mut W {
        self.obj.get_mut()
    }

    // Note that this should only be called if the outer object is just about
    // to be consumed!
    //
    // (e.g. an implementation of `into_inner`)
    pub fn into_inner(self) -> W {
        self.obj.into_inner()
    }

    // pub fn is_present(&self) -> bool {
    //     self.obj.is_some()
    // }

    // Returns total written bytes and status of underlying codec
    pub(crate) fn write_with_status(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<(usize, Status)>> {
        // println!("aw write_with_status");
        // miniz isn't guaranteed to actually write any of the buffer provided,
        // it may be in a flushing mode where it's just giving us data before
        // we're actually giving it any data. We don't want to spuriously return
        // `Ok(0)` when possible as it will cause calls to write_all() to fail.
        // As a result we execute this in a loop to ensure that we try our
        // darndest to write the data.
        loop {
            ready!(self.as_mut().dump(cx))?;

            let mut this = self.project();

            let before_in = this.data.total_in();

            let ret = this.data.run_vec(buf, &mut this.buf, D::Flush::none());

            let written = (this.data.total_in() - before_in) as usize;

            let is_stream_end = match ret {
                Ok(Status::StreamEnd) => true,
                _ => false,
            };

            if buf.len() > 0 && written == 0 && ret.is_ok() && !is_stream_end {
                continue;
            }

            return match ret {
                Ok(st) => match st {
                    Status::Ok | Status::BufError | Status::StreamEnd => {
                        Poll::Ready(Ok((written, st)))
                    }
                },
                Err(..) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "corrupt deflate stream",
                ))),
            };
        }
    }

    #[project]
    fn dump(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        // TODO: should manage this buffer not with `dump` but probably more of
        // a deque-like strategy.
        while !self.buf.is_empty() {
            let this = self.project();

            let n = ready!(this.obj.poll_write(cx, this.buf))?;

            this.buf.drain(..n);

            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
        }

        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite, D: Ops> AsyncWrite for AsyncWriter<W, D> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.as_mut().write_with_status(cx, buf).map_ok(|res| res.0)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        {
            let mut this = self.project();

            this.data
                .run_vec(&[], &mut this.buf, D::Flush::sync())
                .unwrap();
        }

        loop {
            ready!(self.as_mut().dump(cx))?;
            let mut this = self.project();
            let before = this.data.total_out();

            this.data
                .run_vec(&[], &mut this.buf, D::Flush::none())
                .unwrap();

            if before == this.data.total_out() {
                break;
            }
        }

        self.project().obj.poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        ready!(self.as_mut().finish(cx))?;
        self.project().obj.poll_shutdown(cx)
    }
}
