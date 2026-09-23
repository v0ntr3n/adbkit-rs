//! Removes the `\r` that the ADB shell inserts before every `\n`.
//!
//! Ported from `src/adb/linetransform.ts`. Sadly, the ADB shell is not very
//! smart: it automatically converts every `0x0a` (`\n`) it can find to
//! `0x0d 0x0a` (`\r\n`), and it does so for binary content too. We could get
//! rid of this by setting `stty raw`, but that is not available by default.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

/// A stream filter that strips the carriage returns added by the ADB shell.
pub struct LineTransform<S> {
    inner: S,
    output: VecDeque<u8>,
    saved_r: Option<u8>,
    auto_detect: bool,
    transform_needed: bool,
    skip_bytes: usize,
    inner_eof: bool,
}

impl<S> LineTransform<S>
where
    S: AsyncRead + Unpin,
{
    /// Creates a new transform.
    pub fn new(inner: S) -> Self {
        LineTransform::with_options(inner, false)
    }

    /// Creates a new transform with auto detection.
    ///
    /// When enabled, the first byte is inspected: if it is `0x0a` no transform
    /// is needed and the byte is dropped, otherwise one or two bytes are
    /// skipped. This is used for `screencap` and `logcat`, which start with a
    /// PNG magic byte or an `\n`.
    pub fn with_options(inner: S, auto_detect: bool) -> Self {
        LineTransform {
            inner,
            output: VecDeque::new(),
            saved_r: None,
            auto_detect,
            transform_needed: true,
            skip_bytes: 0,
            inner_eof: false,
        }
    }

    /// Feeds bytes into the transform without reading from the inner stream.
    ///
    /// Used by the `screencap` command, which reads the first byte eagerly so
    /// that it can detect devices without support for the command.
    pub fn feed(&mut self, chunk: &[u8]) {
        self.transform(chunk);
    }

    fn transform(&mut self, chunk: &[u8]) {
        let mut chunk = chunk;

        // If auto detection is enabled, check the first byte. The first two
        // bytes must be either 0x0a .. or 0x0d 0x0a. This causes a need to
        // skip either one or two bytes. The autodetection runs only once.
        if self.auto_detect {
            if chunk.first() == Some(&0x0a) {
                self.transform_needed = false;
                self.skip_bytes = 1;
            } else {
                self.skip_bytes = 2;
            }
            self.auto_detect = false;
        }

        // It's technically possible that we may receive the first two bytes in
        // two separate chunks. That's why the autodetect bytes are skipped
        // here, separately.
        if self.skip_bytes > 0 {
            let skip = chunk.len().min(self.skip_bytes);
            chunk = &chunk[skip..];
            self.skip_bytes -= skip;
        }

        if chunk.is_empty() {
            // It's possible that skipping bytes has created an empty chunk.
            return;
        }

        if !self.transform_needed {
            // At this point all bytes that needed to be skipped should have
            // been skipped. If transform is not needed, shortcut to null
            // transform.
            self.output.extend(chunk.iter().copied());
            return;
        }

        // Ok looks like we're transforming.
        let mut lo = 0usize;
        let mut hi = 0usize;
        if let Some(saved_r) = self.saved_r.take() {
            if chunk[0] != 0x0a {
                self.output.push_back(saved_r);
            }
        }
        let last = chunk.len() - 1;
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

    /// Flushes a trailing `\r` that was held back.
    ///
    /// Mirrors `LineTransform._flush()`: when the stream ends on an `\r`, it is
    /// output as-is (assume binary data).
    pub fn flush(&mut self) {
        if let Some(saved_r) = self.saved_r.take() {
            self.output.push_back(saved_r);
        }
    }
}

impl<S> AsyncRead for LineTransform<S>
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
                for _ in 0..amt {
                    buf.put_slice(&[self.output.pop_front().unwrap()]);
                }
                return Poll::Ready(Ok(()));
            }

            if self.inner_eof {
                return Poll::Ready(Ok(()));
            }

            let mut chunk = [0u8; 16 * 1024];
            let mut read_buf = ReadBuf::new(&mut chunk);
            match Pin::new(&mut self.inner).poll_read(cx, &mut read_buf) {
                Poll::Ready(Ok(())) => {
                    let filled = read_buf.filled();
                    if filled.is_empty() {
                        self.inner_eof = true;
                        self.flush();
                    } else {
                        let chunk = filled.to_vec();
                        self.transform(&chunk);
                    }
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S> std::fmt::Debug for LineTransform<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineTransform")
            .field("auto_detect", &self.auto_detect)
            .field("transform_needed", &self.transform_needed)
            .field("skip_bytes", &self.skip_bytes)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn transform(chunks: &[&[u8]], auto_detect: bool) -> Vec<u8> {
        use tokio::io::AsyncReadExt;

        let mut all: Vec<u8> = Vec::new();
        for chunk in chunks {
            all.extend_from_slice(chunk);
        }
        let mut transform = LineTransform::with_options(&all[..], auto_detect);
        let mut out = Vec::new();
        transform.read_to_end(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn test_auto_detect_keeps_rest_if_first_byte_is_newline() {
        assert_eq!(transform(&[b"\nbar\r\n"], true).await, b"bar\r\n".to_vec());
    }

    #[tokio::test]
    async fn test_auto_detect_skips_leading_crlf() {
        assert_eq!(transform(&[b"\r\nbar\r\n"], true).await, b"bar\n".to_vec());
    }

    #[tokio::test]
    async fn test_auto_detect_skips_leading_crlf_in_separate_chunks() {
        assert_eq!(
            transform(&[b"\r", b"\nbar\r\n"], true).await,
            b"bar\n".to_vec()
        );
    }

    #[tokio::test]
    async fn test_auto_detect_transforms_as_usual_if_first_byte_is_not_newline() {
        assert_eq!(
            transform(&[b"\r\nbar\r\nfoo"], true).await,
            b"bar\nfoo".to_vec()
        );
    }

    #[tokio::test]
    async fn test_without_auto_detect_transforms_as_usual() {
        assert_eq!(
            transform(&[b"\n\r\nbar\r\nfoo"], false).await,
            b"\n\nbar\nfoo".to_vec()
        );
    }

    #[tokio::test]
    async fn test_does_not_modify_data_without_crlf() {
        assert_eq!(transform(&[b"foo"], false).await, b"foo".to_vec());
    }

    #[tokio::test]
    async fn test_does_not_remove_cr_if_not_followed_by_lf() {
        assert_eq!(transform(&[&[0x0d, 0x05]], false).await, vec![0x0d, 0x05]);
    }

    #[tokio::test]
    async fn test_removes_cr_if_followed_by_lf() {
        assert_eq!(
            transform(&[&[0x0d, 0x0a, 0x97]], false).await,
            vec![0x0a, 0x97]
        );
    }

    #[tokio::test]
    async fn test_pushes_cr_if_last_in_stream() {
        assert_eq!(transform(&[&[0x0d]], false).await, vec![0x0d]);
    }

    #[tokio::test]
    async fn test_pushes_saved_cr_if_next_chunk_does_not_start_with_lf() {
        assert_eq!(
            transform(&[&[0x62, 0x0d], &[0x37]], false).await,
            vec![0x62, 0x0d, 0x37]
        );
    }

    #[tokio::test]
    async fn test_removes_saved_cr_if_next_chunk_starts_with_lf() {
        assert_eq!(
            transform(&[&[0x62, 0x0d], &[0x0a]], false).await,
            vec![0x62, 0x0a]
        );
    }

    #[tokio::test]
    async fn test_feed() {
        use tokio::io::AsyncReadExt;

        let mut transform = LineTransform::new(&b"bar"[..]);
        transform.feed(b"\r\n");
        let mut out = Vec::new();
        transform.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"\nbar".to_vec());
    }
}
