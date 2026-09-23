//! A TCP server that provides a USB device over TCP.
//!
//! Ported from `src/adb/tcpusb/server.ts`.

use std::sync::Arc;

use std::sync::Arc as StdArc;

use tokio::net::{TcpListener, ToSocketAddrs};
use tokio::sync::mpsc;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::tcpusb::socket::SocketConnection;
use crate::types::SocketOptions;

/// An event emitted by a [`TcpUsbServer`].
#[derive(Debug)]
pub enum TcpUsbEvent {
    /// The server is listening.
    Listening,
    /// A client connected.
    Connection,
    /// The server was closed.
    Close,
    /// An error occurred.
    Error(Error),
}

/// An ADB server, implemented by this crate, that provides a device over TCP.
///
/// This is the equivalent of running `adb usb-device-to-tcp` from the command
/// line: an `adb` client can connect to the server with `adb connect`, and
/// every service it requests is forwarded to the device through a real ADB
/// server.
///
/// Ported from the `Server` class of `src/adb/tcpusb/server.ts`.
pub struct TcpUsbServer {
    client: Arc<Client>,
    serial: String,
    options: SocketOptions,
    listener: Option<StdArc<TcpListener>>,
    events: mpsc::Sender<TcpUsbEvent>,
    events_rx: Option<mpsc::Receiver<TcpUsbEvent>>,
    close: Arc<tokio::sync::Notify>,
}

impl TcpUsbServer {
    /// Creates a new server.
    pub fn new(client: Client, serial: String, options: SocketOptions) -> Self {
        let (tx, rx) = mpsc::channel(64);
        TcpUsbServer {
            client: Arc::new(client),
            serial,
            options,
            listener: None,
            events: tx,
            events_rx: Some(rx),
            close: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Starts listening on the given port.
    ///
    /// Returns the port that ended up being used, which is useful when binding
    /// to port 0.
    pub async fn listen(&mut self, port: u16) -> Result<u16> {
        if let Some(port) = self.local_port() {
            return Ok(port);
        }
        self.listen_addr(("0.0.0.0", port)).await
    }

    /// Starts listening on the given address.
    pub async fn listen_addr(&mut self, addr: impl ToSocketAddrs) -> Result<u16> {
        let listener = StdArc::new(TcpListener::bind(addr).await?);
        let port = listener.local_addr()?.port();
        self.listener = Some(StdArc::clone(&listener));
        let _ = self.events.send(TcpUsbEvent::Listening).await;
        self.spawn_accept_loop(listener);
        Ok(port)
    }

    /// The port the server is listening on, if any.
    pub fn local_port(&self) -> Option<u16> {
        self.listener
            .as_ref()
            .and_then(|listener| listener.local_addr().ok())
            .map(|addr| addr.port())
    }

    /// The events emitted by the server, e.g. connections and errors.
    ///
    /// The receiver can only be handed out once.
    pub fn events(&mut self) -> mpsc::Receiver<TcpUsbEvent> {
        self.events_rx.take().unwrap_or_else(|| mpsc::channel(64).1)
    }

    /// Ends every open connection and stops listening.
    pub async fn end(&mut self) {
        self.close.notify_waiters();
        self.listener = None;
        let _ = self.events.send(TcpUsbEvent::Close).await;
    }

    fn spawn_accept_loop(&self, listener: StdArc<TcpListener>) {
        let client = Arc::clone(&self.client);
        let serial = self.serial.clone();
        let options = self.options.clone();
        let events = self.events.clone();
        let close = Arc::clone(&self.close);

        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    accepted = listener.accept() => accepted,
                    _ = close.notified() => break,
                };
                let (stream, _addr) = match accepted {
                    Ok(accepted) => accepted,
                    Err(err) => {
                        let _ = events.send(TcpUsbEvent::Error(Error::Io(err))).await;
                        continue;
                    }
                };

                let _ = events.send(TcpUsbEvent::Connection).await;
                let connection = SocketConnection::new(
                    Arc::clone(&client),
                    serial.clone(),
                    stream,
                    options.clone(),
                );
                let events_clone = events.clone();
                tokio::spawn(async move {
                    connection.run().await;
                    let _ = events_clone.send(TcpUsbEvent::Close).await;
                });
            }
            let _ = events.send(TcpUsbEvent::Close).await;
        });
    }
}

impl std::fmt::Debug for TcpUsbServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpUsbServer")
            .field("serial", &self.serial)
            .field("local_port", &self.local_port())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_listen_and_close() {
        let client = Client::new(crate::types::ClientOptions::default());
        let mut server = TcpUsbServer::new(client, "serial".to_string(), SocketOptions::default());
        assert_eq!(server.local_port(), None);

        let port = server.listen(0).await.unwrap();
        assert!(port > 0);
        assert_eq!(server.local_port(), Some(port));
        assert_eq!(server.listen(0).await.unwrap(), port);

        let _events = server.events();
        server.end().await;
        assert_eq!(server.local_port(), None);
    }
}
