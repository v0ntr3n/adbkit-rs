//! Reads ADB TCP protocol packets from a stream.
//!
//! Ported from `src/adb/tcpusb/packetreader.ts`.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use crate::error::{Error, Result};
use crate::tcpusb::packet::{Packet, HEADER_SIZE};

/// Reads packets from a stream.
///
/// Ported from the `PacketReader` class of `src/adb/tcpusb/packetreader.ts`.
/// The TypeScript original is an `EventEmitter`; here the packets are either
/// polled with [`PacketReader::next`] or consumed as a [`futures::Stream`].
pub struct PacketReader<R> {
    stream: R,
    buffer: Vec<u8>,
    in_body: bool,
    packet: Option<Packet>,
}

impl<R> PacketReader<R>
where
    R: AsyncRead + Unpin,
{
    /// Creates a reader for the given stream.
    pub fn new(stream: R) -> Self {
        PacketReader {
            stream,
            buffer: Vec::new(),
            in_body: false,
            packet: None,
        }
    }

    /// Waits for the next packet.
    ///
    /// Returns `None` once the stream ends.
    pub async fn next(&mut self) -> Option<Result<Packet>> {
        loop {
            match self.try_read() {
                Ok(Some(packet)) => return Some(Ok(packet)),
                Ok(None) => {}
                Err(err) => return Some(Err(err)),
            }

            let mut chunk = [0u8; 64 * 1024];
            match self.stream.read(&mut chunk).await {
                Ok(0) => return None,
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                Err(err) => return Some(Err(Error::Io(err))),
            }
        }
    }

    /// Reads as many packets as are currently buffered.
    pub fn buffered_packets(&mut self) -> Vec<Result<Packet>> {
        let mut packets = Vec::new();
        loop {
            match self.try_read() {
                Ok(Some(packet)) => packets.push(Ok(packet)),
                Ok(None) => break,
                Err(err) => {
                    packets.push(Err(err));
                    break;
                }
            }
        }
        packets
    }

    /// The bytes that are buffered while waiting for a complete packet.
    pub fn buffered(&self) -> &[u8] {
        &self.buffer
    }

    /// Parses complete packets from the buffer.
    fn try_read(&mut self) -> Result<Option<Packet>> {
        while !self.buffer.is_empty() {
            if self.in_body {
                let length = self
                    .packet
                    .as_ref()
                    .map(|packet| packet.length)
                    .unwrap_or(0) as usize;
                if self.buffer.len() < length {
                    return Ok(None);
                }
                let data: Vec<u8> = self.buffer.drain(..length).collect();
                let mut packet = self.packet.take().expect("a packet is in progress");
                packet.data = Some(data);
                self.in_body = false;
                if !packet.verify_checksum() {
                    return Err(Error::Checksum);
                }
                return Ok(Some(packet));
            } else {
                if self.buffer.len() < HEADER_SIZE {
                    return Ok(None);
                }
                let header: Vec<u8> = self.buffer.drain(..HEADER_SIZE).collect();
                let read_u32 = |offset: usize| {
                    u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap())
                };
                let packet = Packet {
                    command: read_u32(0),
                    arg0: read_u32(4),
                    arg1: read_u32(8),
                    length: read_u32(12),
                    check: read_u32(16),
                    magic: read_u32(20),
                    data: Some(Vec::new()),
                };
                if !packet.verify_magic() {
                    return Err(Error::Magic);
                }
                if packet.length == 0 {
                    return Ok(Some(packet));
                }
                self.packet = Some(packet);
                self.in_body = true;
            }
        }
        Ok(None)
    }

    /// Polls for the next packet.
    fn poll_next_packet(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Packet>>> {
        loop {
            match self.try_read() {
                Ok(Some(packet)) => return Poll::Ready(Some(Ok(packet))),
                Ok(None) => {}
                Err(err) => return Poll::Ready(Some(Err(err))),
            }

            let mut chunk = [0u8; 64 * 1024];
            let mut read = ReadBuf::new(&mut chunk);
            match Pin::new(&mut self.stream).poll_read(cx, &mut read) {
                Poll::Ready(Ok(())) => {
                    let filled = read.filled().to_vec();
                    if filled.is_empty() {
                        return Poll::Ready(None);
                    }
                    self.buffer.extend_from_slice(&filled);
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(Error::Io(err)))),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<R> futures::Stream for PacketReader<R>
where
    R: AsyncRead + Unpin,
{
    type Item = Result<Packet>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.poll_next_packet(cx)
    }
}

impl<R> std::fmt::Debug for PacketReader<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PacketReader")
            .field("buffered", &self.buffer.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcpusb::packet::{self, A_CLSE, A_CNXN, A_OKAY, A_WRTE};

    #[tokio::test]
    async fn test_reads_a_packet() {
        let data = Packet::assemble(A_OKAY, 1, 2, None);
        let mut reader = PacketReader::new(&data[..]);
        let packet = reader.next().await.unwrap().unwrap();
        assert_eq!(packet.command, A_OKAY);
        assert_eq!(packet.arg0, 1);
        assert_eq!(packet.arg1, 2);
        assert_eq!(packet.length, 0);
        assert!(reader.next().await.is_none());
    }

    #[tokio::test]
    async fn test_reads_a_packet_with_payload() {
        let data = Packet::assemble(A_WRTE, 1, 2, Some(b"hello"));
        let mut reader = PacketReader::new(&data[..]);
        let packet = reader.next().await.unwrap().unwrap();
        assert_eq!(packet.command, A_WRTE);
        assert_eq!(packet.data.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn test_reads_multiple_packets() {
        let mut data = Packet::assemble(A_OKAY, 1, 2, None);
        data.extend(Packet::assemble(A_CLSE, 1, 2, None));
        let mut reader = PacketReader::new(&data[..]);
        assert_eq!(reader.next().await.unwrap().unwrap().command, A_OKAY);
        assert_eq!(reader.next().await.unwrap().unwrap().command, A_CLSE);
        assert!(reader.next().await.is_none());
    }

    #[tokio::test]
    async fn test_waits_for_incomplete_packets() {
        let data = Packet::assemble(A_WRTE, 1, 2, Some(b"hello"));
        let (mut client, mut server) = tokio::io::duplex(1024);
        let mut reader = PacketReader::new(&mut client);

        // Only the header, without the payload.
        use tokio::io::AsyncWriteExt;
        server.write_all(&data[..HEADER_SIZE]).await.unwrap();
        server.write_all(&data[HEADER_SIZE..]).await.unwrap();

        let packet = reader.next().await.unwrap().unwrap();
        assert_eq!(packet.data.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn test_rejects_bad_magic() {
        let mut data = Packet::assemble(A_OKAY, 1, 2, None);
        data[20] ^= 0xff;
        let mut reader = PacketReader::new(&data[..]);
        assert_eq!(reader.next().await.unwrap().unwrap_err(), Error::Magic);
    }

    #[tokio::test]
    async fn test_rejects_bad_checksum() {
        let mut data = Packet::assemble(A_WRTE, 1, 2, Some(b"hello"));
        let last = data.len() - 1;
        data[last] ^= 0xff;
        let mut reader = PacketReader::new(&data[..]);
        assert_eq!(reader.next().await.unwrap().unwrap_err(), Error::Checksum);
    }

    #[tokio::test]
    async fn test_stream_impl() {
        let mut data = Packet::assemble(A_CNXN, 1, 2, Some(b"device::"));
        data.extend(Packet::assemble(A_OKAY, 3, 4, None));
        let mut reader = PacketReader::new(&data[..]);
        let mut commands = Vec::new();
        while let Some(packet) = reader.next().await {
            commands.push(packet.unwrap().command);
        }
        assert_eq!(commands, vec![packet::A_CNXN, packet::A_OKAY]);
    }
}
