//! The `monkey` module: a client for Android's `monkey` utility.
//!
//! Ported from the `@devicefarmer/adbkit-monkey` package (`lib/monkey/*.js`).

pub mod api;
pub mod client;
pub mod parser;
pub mod queue;
pub mod reply;

pub use api::Api;
pub use client::{connect, connect_stream, Client, ClientEvent, ConnectOptions, Multi};
pub use parser::Parser;
pub use queue::Queue;
pub use reply::{Reply, ReplyType};

/// The `monkey` facade.
///
/// Ported from `lib/monkey.js`.
pub struct Monkey;

impl Monkey {
    /// Connects to a `monkey` utility over TCP.
    ///
    /// Ported from `Monkey.connect()`.
    pub async fn connect(options: ConnectOptions) -> Result<Client<tokio::net::TcpStream>> {
        client::connect(options).await
    }

    /// Connects to a `monkey` utility using an already established stream.
    ///
    /// Ported from `Monkey.connectStream()`.
    pub fn connect_stream<S: AdbStream>(stream: S) -> Client<S> {
        client::connect_stream(stream)
    }
}

use crate::connection::AdbStream;
use crate::error::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_stream_sends_commands() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut monkey = Monkey::connect_stream(client);

        // Nothing answers, so the command itself must still be written.
        let sent =
            tokio::time::timeout(std::time::Duration::from_millis(200), monkey.tap(10, 20)).await;
        assert!(sent.is_err(), "no reply is expected");

        let mut buf = vec![0u8; 32];
        let read = tokio::io::AsyncReadExt::read(&mut server, &mut buf)
            .await
            .unwrap();
        assert_eq!(&buf[..read], b"tap 10 20\n");
    }
}
