//! Framebuffer support: metadata parsing, raw to RGB conversion and the
//! stream returned by `DeviceClient::framebuffer()`.
//!
//! Ported from `src/adb/command/host-transport/framebuffer.ts` and
//! `src/adb/framebuffer/rgbtransform.ts`.

use std::process::Stdio;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};
use tokio::process;

use crate::connection::{AdbStream, RawStream};
use crate::error::{Error, Result};
use crate::types::{ColorFormat, FramebufferMeta};

/// The size of a framebuffer header, in bytes.
pub const HEADER_SIZE: usize = 52;

/// Transforms a raw BGR/BGRA framebuffer into RGB.
///
/// Ported from `RgbTransform`. Only 24-bit and 32-bit raw images with 8 bits
/// per color are supported.
pub struct RgbTransform {
    buffer: Vec<u8>,
    r_pos: usize,
    g_pos: usize,
    b_pos: usize,
    pixel_bytes: usize,
}

impl RgbTransform {
    /// Creates a transform for the given framebuffer metadata.
    pub fn new(meta: &FramebufferMeta) -> Result<Self> {
        if meta.bpp != 24 && meta.bpp != 32 {
            return Err(Error::Device(
                "Only 24-bit and 32-bit raw images with 8-bits per color are supported".to_string(),
            ));
        }
        Ok(RgbTransform {
            buffer: Vec::new(),
            r_pos: (meta.red_offset / 8) as usize,
            g_pos: (meta.green_offset / 8) as usize,
            b_pos: (meta.blue_offset / 8) as usize,
            pixel_bytes: (meta.bpp / 8) as usize,
        })
    }

    /// Transforms a chunk of raw data, returning the RGB bytes produced.
    ///
    /// Incomplete pixels are kept until the rest arrives.
    pub fn transform(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.buffer.extend_from_slice(chunk);
        let mut source_cursor = 0usize;
        let mut target: Vec<u8> = Vec::with_capacity(self.buffer.len() / self.pixel_bytes * 3);
        while self.buffer.len() - source_cursor >= self.pixel_bytes {
            let r = self.buffer[source_cursor + self.r_pos];
            let g = self.buffer[source_cursor + self.g_pos];
            let b = self.buffer[source_cursor + self.b_pos];
            target.push(r);
            target.push(g);
            target.push(b);
            source_cursor += self.pixel_bytes;
        }
        if !target.is_empty() {
            self.buffer.drain(..source_cursor);
        }
        target
    }

    /// The number of buffered bytes that do not form a complete pixel yet.
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

/// The raw framebuffer, possibly converted into another format.
///
/// Returned by `DeviceClient::framebuffer()`; the stream is readable and
/// carries the framebuffer [`FramebufferMeta`], mirroring
/// `FramebufferStreamWithMeta`.
pub struct FramebufferStream<S>
where
    S: AdbStream,
{
    inner: FramebufferInner<S>,
    /// The framebuffer metadata.
    pub meta: FramebufferMeta,
}

enum FramebufferInner<S>
where
    S: AdbStream,
{
    Raw(RawStream<S>),
    Converted(process::ChildStdout),
}

impl<S> FramebufferStream<S>
where
    S: AdbStream,
{
    /// Creates a raw (unconverted) framebuffer stream.
    pub fn raw(stream: RawStream<S>, meta: FramebufferMeta) -> Self {
        FramebufferStream {
            inner: FramebufferInner::Raw(stream),
            meta,
        }
    }

    /// Converts the raw framebuffer into another format using GraphicsMagick.
    ///
    /// Mirrors `FrameBufferCommand._convert()`: the raw stream is piped into
    /// `gm convert -size <width>x<height> <format>:- <output>:-`, first being
    /// transformed into `rgb` if the device does not use that layout.
    pub async fn convert(
        meta: FramebufferMeta,
        format: &str,
        stream: RawStream<S>,
    ) -> Result<Self> {
        crate::debug!(
            "Converting raw framebuffer stream into {}",
            format.to_uppercase()
        );

        let (input_format, mut transform) = match meta.format {
            ColorFormat::Rgb | ColorFormat::Rgba => (meta.format.as_str().to_string(), None),
            other => {
                crate::debug!("Silently transforming '{}' into 'rgb' for `gm`", other);
                let transform = RgbTransform::new(&meta)?;
                ("rgb".to_string(), Some(transform))
            }
        };

        let mut command = process::Command::new("gm");
        command
            .arg("convert")
            .arg("-size")
            .arg(format!("{}x{}", meta.width, meta.height))
            .arg(format!("{}:-", input_format))
            .arg(format!("{}:-", format))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Device("Unable to pipe into `gm`".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Device("Unable to read from `gm`".to_string()))?;

        // Pump the raw stream into `gm` in the background.
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;

            let mut stream = stream;
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(read) => {
                        let data = match transform.as_mut() {
                            Some(transform) => transform.transform(&buf[..read]),
                            None => buf[..read].to_vec(),
                        };
                        if stdin.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = stdin.shutdown().await;
            let _ = child.wait().await;
        });

        Ok(FramebufferStream {
            inner: FramebufferInner::Converted(stdout),
            meta,
        })
    }
}

impl<S> AsyncRead for FramebufferStream<S>
where
    S: AdbStream,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut self.inner {
            FramebufferInner::Raw(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            FramebufferInner::Converted(stdout) => std::pin::Pin::new(stdout).poll_read(cx, buf),
        }
    }
}

impl<S> std::fmt::Debug for FramebufferStream<S>
where
    S: AdbStream,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FramebufferStream")
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

/// Parses a 52 byte framebuffer header.
///
/// Ported from `FrameBufferCommand._parseHeader()`.
pub fn parse_header(header: &[u8]) -> Result<FramebufferMeta> {
    let mut offset = 0usize;
    let next = |offset: &mut usize| {
        let value = u32::from_le_bytes([
            header.get(*offset).copied().unwrap_or(0),
            header.get(*offset + 1).copied().unwrap_or(0),
            header.get(*offset + 2).copied().unwrap_or(0),
            header.get(*offset + 3).copied().unwrap_or(0),
        ]);
        *offset += 4;
        value
    };

    let version = next(&mut offset);
    if version == 16 {
        return Err(Error::Device(
            "Old-style raw images are not supported".to_string(),
        ));
    }
    let bpp = next(&mut offset);
    let size = next(&mut offset);
    let width = next(&mut offset);
    let height = next(&mut offset);
    let red_offset = next(&mut offset);
    let red_length = next(&mut offset);
    let blue_offset = next(&mut offset);
    let blue_length = next(&mut offset);
    let green_offset = next(&mut offset);
    let green_length = next(&mut offset);
    let alpha_offset = next(&mut offset);
    let alpha_length = next(&mut offset);

    let mut format = if blue_offset == 0 {
        ColorFormat::Bgr
    } else {
        ColorFormat::Rgb
    };
    if bpp == 32 || alpha_length > 0 {
        format = match format {
            ColorFormat::Bgr => ColorFormat::Bgra,
            ColorFormat::Bgra => ColorFormat::Bgra,
            ColorFormat::Rgb => ColorFormat::Rgba,
            ColorFormat::Rgba => ColorFormat::Rgba,
        };
    }

    Ok(FramebufferMeta {
        version,
        format,
        width,
        height,
        bpp,
        size,
        red_offset,
        red_length,
        blue_offset,
        blue_length,
        green_offset,
        green_length,
        alpha_offset,
        alpha_length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(
        bpp: u32,
        red_offset: u32,
        green_offset: u32,
        blue_offset: u32,
        alpha_offset: u32,
        alpha_length: u32,
    ) -> FramebufferMeta {
        FramebufferMeta {
            version: 1,
            format: ColorFormat::Rgb,
            width: 1080,
            height: 1920,
            bpp,
            size: 0,
            red_offset,
            red_length: 8,
            blue_offset,
            blue_length: 8,
            green_offset,
            green_length: 8,
            alpha_offset,
            alpha_length,
        }
    }

    #[test]
    fn test_transform_bgra_into_rgb() {
        let meta = meta(32, 16, 8, 0, 24, 8);
        let mut transform = RgbTransform::new(&meta).unwrap();
        let out = transform.transform(&[50, 100, 150, 200]);
        assert_eq!(out, vec![150, 100, 50]);
        assert_eq!(transform.buffered(), 0);
    }

    #[test]
    fn test_transform_bgr_into_rgb() {
        let meta = meta(32, 16, 8, 0, 0, 0);
        let mut transform = RgbTransform::new(&meta).unwrap();
        let out = transform.transform(&[50, 100, 150, 0]);
        assert_eq!(out, vec![150, 100, 50]);
    }

    #[test]
    fn test_transform_rgb_into_rgb() {
        let meta = meta(24, 0, 8, 16, 0, 0);
        let mut transform = RgbTransform::new(&meta).unwrap();
        let out = transform.transform(&[50, 100, 150]);
        assert_eq!(out, vec![50, 100, 150]);
    }

    #[test]
    fn test_transform_rgba_into_rgb() {
        let meta = meta(32, 0, 8, 16, 24, 8);
        let mut transform = RgbTransform::new(&meta).unwrap();
        let out = transform.transform(&[50, 100, 150, 200]);
        assert_eq!(out, vec![50, 100, 150]);
    }

    #[test]
    fn test_waits_for_complete_pixel() {
        let meta = meta(32, 0, 8, 16, 24, 8);
        let mut transform = RgbTransform::new(&meta).unwrap();
        assert!(transform.transform(&[50, 100]).is_empty());
        assert!(transform.transform(&[150]).is_empty());
        assert_eq!(transform.transform(&[200]), vec![50, 100, 150]);
    }

    #[test]
    fn test_transforms_multiple_pixels() {
        let meta = meta(32, 16, 8, 0, 24, 8);
        let mut transform = RgbTransform::new(&meta).unwrap();
        let pixel1 = [50, 100, 150, 200];
        let pixel2 = [51, 101, 151, 201];
        let mut all = Vec::new();
        for _ in 0..5 {
            let pixel = if all.len() % 2 == 0 { pixel1 } else { pixel2 };
            all.extend(transform.transform(&pixel));
        }
        assert_eq!(all.len(), 15);
        assert_eq!(&all[0..3], &[150, 100, 50]);
        assert_eq!(&all[3..6], &[151, 101, 51]);
    }

    #[test]
    fn test_rejects_unsupported_bpp() {
        let meta = meta(16, 0, 8, 16, 0, 0);
        assert!(RgbTransform::new(&meta).is_err());
    }

    #[test]
    fn test_parse_header() {
        let mut header = vec![0u8; HEADER_SIZE];
        let mut write = |offset: usize, value: u32| {
            header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        };
        write(0, 1); // version
        write(4, 32); // bpp
        write(8, 1024); // size
        write(12, 1080); // width
        write(16, 1920); // height
        write(20, 16); // red_offset
        write(24, 8); // red_length
        write(28, 0); // blue_offset
        write(32, 8); // blue_length
        write(36, 8); // green_offset
        write(40, 8); // green_length
        write(44, 24); // alpha_offset
        write(48, 8); // alpha_length

        let meta = parse_header(&header).unwrap();
        assert_eq!(meta.version, 1);
        assert_eq!(meta.bpp, 32);
        assert_eq!(meta.size, 1024);
        assert_eq!(meta.width, 1080);
        assert_eq!(meta.height, 1920);
        assert_eq!(meta.format, ColorFormat::Bgra);
    }

    #[test]
    fn test_parse_header_old_version() {
        let mut header = vec![0u8; HEADER_SIZE];
        header[0..4].copy_from_slice(&16u32.to_le_bytes());
        assert!(parse_header(&header).is_err());
    }
}
