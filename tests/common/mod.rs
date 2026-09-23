//! Test helpers.
//!
//! Ported from `test/mock/duplex.ts` and `test/mock/connection.ts`. The mocha
//! suite feeds bytes into a mock duplex and observes what the client wrote;
//! `tokio::io::duplex` provides exactly that in Rust.

use adbkit::protocol::Protocol;
use adbkit::{ClientOptions, Connection};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

/// The server side of a mock connection.
pub struct Mock {
    server: DuplexStream,
}

/// Creates a mock connection, together with the server side used by the tests.
pub fn mock() -> (Mock, Connection<DuplexStream>) {
    let (client, server) = tokio::io::duplex(256 * 1024);
    (
        Mock { server },
        Connection::new(client, ClientOptions::default()),
    )
}

impl Mock {
    /// Feeds raw bytes to the client.
    pub async fn feed(&mut self, data: &[u8]) {
        self.server.write_all(data).await.expect("feed failed");
    }

    /// Feeds an `OKAY` reply.
    pub async fn feed_okay(&mut self) {
        self.feed(b"OKAY").await;
    }

    /// Feeds a `FAIL` reply with the given message.
    pub async fn feed_fail(&mut self, message: &str) {
        self.feed(b"FAIL").await;
        self.feed_value(message.as_bytes()).await;
    }

    /// Feeds a length-prefixed value, as the ADB protocol requires.
    pub async fn feed_value(&mut self, value: &[u8]) {
        self.feed(&Protocol::encode_data(value)).await;
    }

    /// Ends the stream, like `MockDuplex.causeEnd()`.
    pub async fn end(&mut self) {
        self.server.shutdown().await.expect("shutdown failed");
    }

    /// Reads everything the client has written so far.
    pub async fn written(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(50),
                self.server.read(&mut buf),
            )
            .await
            {
                Ok(Ok(0)) => break,
                Ok(Ok(read)) => {
                    out.extend_from_slice(&buf[..read]);
                    if read < buf.len() {
                        break;
                    }
                }
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }
        out
    }
}

/// Asserts that the client wrote exactly the given (protocol encoded) data.
#[allow(dead_code)]
pub async fn assert_written(mock: &mut Mock, expected: &str) {
    let expected = Protocol::encode_data(expected);
    let written = mock.written().await;
    assert_eq!(
        String::from_utf8_lossy(&written),
        String::from_utf8_lossy(&expected),
        "unexpected data written to the connection"
    );
}

/// Reads a whole stream, as `adb.util.readAll()` does.
#[allow(dead_code)]
pub async fn read_all<R: tokio::io::AsyncRead + Unpin>(
    stream: R,
) -> Result<Vec<u8>, std::io::Error> {
    let mut stream = stream;
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await?;
    Ok(out)
}
