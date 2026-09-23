//! A connection to the ADB server.
//!
//! Ported from `src/adb/connection.ts`. The TypeScript original emits
//! `connect`, `end`, `drain`, `timeout`, `close` and `error` events; since the
//! Rust API is `async`, those events map onto the returned futures instead.

use std::io;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf};
use tokio::net::TcpStream;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::parser::Parser;
use crate::types::ClientOptions;

/// Anything that can act as the transport for an ADB connection.
///
/// This is the Rust equivalent of Node's duplex `net.Socket`: a TCP stream in
/// production, an in-memory duplex stream in tests.
pub trait AdbStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {}

impl<T> AdbStream for T where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static
{
}

/// A stream that can be read from and written to, passed through from the ADB
/// server to the caller.
///
/// Returned by `DeviceClient::shell()`, `openLocal()`, `openTcp()`, `openLog()`,
/// `openLogcat()` and friends, mirroring the `Duplex` streams in the
/// TypeScript implementation.
pub struct RawStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    pending: io::Cursor<Vec<u8>>,
    reader: ReadHalf<S>,
    writer: WriteHalf<S>,
}

impl<S> RawStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// Creates a new stream from both halves of a connection, plus any bytes
    /// that the parser had already buffered.
    pub fn new(pending: Vec<u8>, reader: ReadHalf<S>, writer: WriteHalf<S>) -> Self {
        RawStream {
            pending: io::Cursor::new(pending),
            reader,
            writer,
        }
    }

    /// A mutable reference to the read half.
    pub fn reader(&mut self) -> &mut ReadHalf<S> {
        &mut self.reader
    }

    /// A mutable reference to the write half.
    pub fn writer(&mut self) -> &mut WriteHalf<S> {
        &mut self.writer
    }
}

impl<S> tokio::io::AsyncRead for RawStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        if self.pending.position() < self.pending.get_ref().len() as u64 {
            let pending = self.pending.get_ref();
            let start = self.pending.position() as usize;
            let end = pending.len();
            let remaining = &pending[start..end];
            let amt = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..amt]);
            self.pending.set_position((start + amt) as u64);
            return std::task::Poll::Ready(Ok(()));
        }
        std::pin::Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl<S> tokio::io::AsyncWrite for RawStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        std::pin::Pin::new(&mut self.writer).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

impl<S> std::fmt::Debug for RawStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawStream").finish_non_exhaustive()
    }
}

/// A connection to the ADB server.
///
/// Connections are cheap: the TypeScript implementation opens a new one for
/// every logical group of commands, and this port does the same.
pub struct Connection<S = TcpStream>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    parser: Parser<ReadHalf<S>>,
    writer: WriteHalf<S>,
    /// The options this connection was created with.
    pub options: ClientOptions,
}

impl<S> Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// Wraps an already established stream.
    pub fn new(stream: S, options: ClientOptions) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        let timeout = if options.timeout == 0 {
            None
        } else {
            Some(Duration::from_millis(options.timeout))
        };
        Connection {
            parser: Parser::new(reader, timeout),
            writer,
            options,
        }
    }

    /// A mutable reference to the parser.
    pub fn parser(&mut self) -> &mut Parser<ReadHalf<S>> {
        &mut self.parser
    }

    /// A mutable reference to the parser, for reading raw data.
    pub fn reader_mut(&mut self) -> &mut Parser<ReadHalf<S>> {
        &mut self.parser
    }

    /// Writes data to the connection.
    pub async fn write(&mut self, data: &[u8]) -> Result<()> {
        tokio::io::AsyncWriteExt::write_all(&mut self.writer, data)
            .await
            .map_err(Error::from)
    }

    /// Writes data and flushes.
    pub async fn write_flush(&mut self, data: &[u8]) -> Result<()> {
        self.write(data).await?;
        tokio::io::AsyncWriteExt::flush(&mut self.writer)
            .await
            .map_err(Error::from)
    }

    /// Ends the connection.
    pub async fn end(&mut self) {
        let _ = tokio::io::AsyncWriteExt::shutdown(&mut self.writer).await;
    }

    /// Splits the connection into the underlying stream's halves.
    ///
    /// Any bytes the parser had buffered are discarded, so this should only be
    /// used on connections that have just been handshaked.
    pub fn into_split(self) -> (ReadHalf<S>, WriteHalf<S>) {
        let (reader, _pending) = self.parser.into_parts();
        (reader, self.writer)
    }

    /// Turns the connection into a [`RawStream`], as returned by commands such
    /// as `shell:` or `tcp:`.
    ///
    /// Any bytes the parser had buffered are preserved.
    pub fn into_raw_stream(self) -> Result<RawStream<S>> {
        let (reader, pending) = self.parser.into_parts();
        Ok(RawStream::new(pending, reader, self.writer))
    }
}

impl Connection<tokio::net::TcpStream> {
    /// Connects to the ADB server, starting it first if necessary.
    ///
    /// Mirrors `Connection.connect()`: if the connection is refused and we
    /// haven't tried to start the server yet, we run `adb start-server` and
    /// try again.
    pub async fn connect(client: &Client) -> Result<Self> {
        let options = client.options().clone();
        let addr = format!("{}:{}", client.host(), client.port());
        debug!("Connecting to ADB server at {}", addr);

        let mut tried_starting = false;
        loop {
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    stream.set_nodelay(true).ok();
                    return Ok(Connection::new(stream, options));
                }
                Err(err) => {
                    if err.kind() == io::ErrorKind::ConnectionRefused && !tried_starting {
                        debug!("Connection was refused, let's try starting the server once");
                        tried_starting = true;
                        client.start_server().await?;
                    } else {
                        return Err(err.into());
                    }
                }
            }
        }
    }
}

impl<S> std::fmt::Debug for Connection<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_write_and_read() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut connection = Connection::new(client, ClientOptions::default());

        connection.write_flush(b"0004OKAY").await.unwrap();

        let mut buf = vec![0u8; 32];
        let read = tokio::io::AsyncReadExt::read(&mut server, &mut buf)
            .await
            .unwrap();
        assert_eq!(&buf[..read], b"0004OKAY");

        // Whatever the server sends is readable through the parser.
        server.write_all(b"OKAY").await.unwrap();
        assert_eq!(connection.parser().read_ascii(4).await.unwrap(), "OKAY");
    }

    #[tokio::test]
    async fn test_raw_stream_passes_data_through() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut connection = Connection::new(client, ClientOptions::default());

        use tokio::io::AsyncWriteExt;
        server.write_all(b"OKAYpayload").await.unwrap();
        server.shutdown().await.unwrap();

        // Read the reply code through the parser, then hand over the stream.
        assert_eq!(connection.parser().read_ascii(4).await.unwrap(), "OKAY");
        let mut stream = connection.into_raw_stream().unwrap();

        let mut out = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut out)
            .await
            .unwrap();
        assert_eq!(out, b"payload".to_vec());
    }

    #[tokio::test]
    async fn test_into_split_reunites_the_halves() {
        let (client, mut server) = tokio::io::duplex(1024);
        let connection = Connection::new(client, ClientOptions::default());
        let (mut reader, mut writer) = connection.into_split();

        use tokio::io::AsyncWriteExt;
        writer.write_all(b"ping").await.unwrap();

        let mut buf = vec![0u8; 8];
        let read = tokio::io::AsyncReadExt::read(&mut server, &mut buf)
            .await
            .unwrap();
        assert_eq!(&buf[..read], b"ping");

        server.write_all(b"pong").await.unwrap();
        let read = tokio::io::AsyncReadExt::read(&mut reader, &mut buf)
            .await
            .unwrap();
        assert_eq!(&buf[..read], b"pong");
    }
}
