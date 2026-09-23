//! Removes the `\r` that the ADB shell inserts before every `\n`.
//!
//! Ported from `lib/logcat/transform.js`, which is a verbatim copy of
//! `src/adb/linetransform.ts`.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

/// A stream filter that strips the carriage returns added by the ADB shell.
///
/// Ported from the `Transform` class of `lib/logcat/transform.js`.
pub struct Transform<S> {
    inner: S,
    output: VecDeque<u8>,
    saved_r: Option<u8>,
    inner_eof: bool,
}

impl<S> Transform<S>
where
    S: AsyncRead + Unpin,
{
    /// Creates a new transform.
    pub fn new(inner: S) -> Self {
        Transform {
            inner,
            output: VecDeque::new(),
            saved_r: None,
            inner_eof: false,
        }
    }

    /// Transforms a chunk of data, as `Transform._transform()` does.
    pub fn transform(&mut self, chunk: &[u8]) {
        let mut lo = 0usize;
        let mut hi = 0usize;
        if let Some(saved_r) = self.saved_r.take() {
            if chunk.first() != Some(&0x0a) {
                self.output.push_back(saved_r);
            }
        }
        let last = chunk.len().saturating_sub(1);
        while hi <= last {
            if chunk[hi] == 0x0d {
                if hi == last {
                    self.saved_r = Some(chunk[last]);
                    break; // Stop hi from incrementing, we want to skip the last byte.
                } else if chunk[hi + 1] == 0x0a {
                    self.output.extend(chunk[lo..hi].iter().copied());
                    lo = hi + 1;
                }
            }
            hi += 1;
        }
        if hi != lo {
            self.output.extend(chunk[lo..hi].iter().copied());
        }
    }
}

impl<S> AsyncRead for Transform<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if !self.output.is_empty() {
                let amt = self.output.len().min(buf.remaining());
                let mut chunk = Vec::with_capacity(amt);
                for _ in 0..amt {
                    chunk.push(self.output.pop_front().unwrap());
                }
                buf.put_slice(&chunk);
                return Poll::Ready(Ok(()));
            }

            if self.inner_eof {
                // When the stream ends on an `\r`, output it as-is (assume
                // binary data), exactly like the flush handler of a Node
                // Transform stream.
                if let Some(saved_r) = self.saved_r.take() {
                    self.output.push_back(saved_r);
                    continue;
                }
                return Poll::Ready(Ok(()));
            }

            let mut chunk = [0u8; 16 * 1024];
            let mut read = ReadBuf::new(&mut chunk);
            match Pin::new(&mut self.inner).poll_read(cx, &mut read) {
                Poll::Ready(Ok(())) => {
                    let filled = read.filled().to_vec();
                    if filled.is_empty() {
                        self.inner_eof = true;
                    } else {
                        self.transform(&filled);
                    }
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S> std::fmt::Debug for Transform<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transform").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    async fn transform(chunks: &[&[u8]]) -> Vec<u8> {
        let mut all: Vec<u8> = Vec::new();
        for chunk in chunks {
            all.extend_from_slice(chunk);
        }
        let mut transform = Transform::new(&all[..]);
        let mut out = Vec::new();
        transform.read_to_end(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn test_does_not_modify_data_without_crlf() {
        assert_eq!(transform(&[b"foo"]).await, b"foo".to_vec());
    }

    #[tokio::test]
    async fn test_does_not_remove_cr_if_not_followed_by_lf() {
        assert_eq!(transform(&[&[0x0d, 0x05]]).await, vec![0x0d, 0x05]);
    }

    #[tokio::test]
    async fn test_removes_cr_if_followed_by_lf() {
        assert_eq!(transform(&[&[0x0d, 0x0a, 0x97]]).await, vec![0x0a, 0x97]);
    }

    #[tokio::test]
    async fn test_pushes_cr_if_last_in_stream() {
        assert_eq!(transform(&[&[0x0d]]).await, vec![0x0d]);
    }

    #[tokio::test]
    async fn test_pushes_saved_cr_if_next_chunk_does_not_start_with_lf() {
        assert_eq!(
            transform(&[&[0x62, 0x0d], &[0x37]]).await,
            vec![0x62, 0x0d, 0x37]
        );
    }

    #[tokio::test]
    async fn test_removes_saved_cr_if_next_chunk_starts_with_lf() {
        assert_eq!(transform(&[&[0x62, 0x0d], &[0x0a]]).await, vec![0x62, 0x0a]);
    }
}
