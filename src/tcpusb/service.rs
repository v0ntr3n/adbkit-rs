//! An open service of an ADB connection.
//!
//! Ported from `src/adb/tcpusb/service.ts`.
//!
//! The TypeScript original stores the transport connection in the service and
//! reacts to its `readable`/`end` events. Here the transport is split in two:
//! the read half is owned by a background task that pumps data to the client,
//! while the write half is used when the client sends `A_WRTE` packets. That
//! way no lock is ever held while waiting for data.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Notify};

use crate::client::Client;
use crate::error::{Error, Result};
use crate::tcpusb::packet::{self, Packet};

/// The shared state of a service.
#[derive(Debug, Default)]
struct ServiceState {
    opened: bool,
    ended: bool,
    need_ack: bool,
}

/// A service that forwards data between a client and a device.
///
/// Ported from the `Service` class of `src/adb/tcpusb/service.ts`.
pub struct Service {
    client: Arc<Client>,
    serial: String,
    local_id: u32,
    remote_id: u32,
    /// The client end of the connection.
    client_writer: Arc<Mutex<WriteHalf<TcpStream>>>,
    /// The maximum payload size of the client connection.
    max_payload: u32,
    state: Arc<Mutex<ServiceState>>,
    wake: Arc<Notify>,
    transport_writer: Option<Arc<Mutex<WriteHalf<TcpStream>>>>,
}

impl Service {
    /// Creates a new service.
    pub fn new(
        client: Arc<Client>,
        serial: String,
        local_id: u32,
        remote_id: u32,
        client_writer: Arc<Mutex<WriteHalf<TcpStream>>>,
        max_payload: u32,
    ) -> Self {
        Service {
            client,
            serial,
            local_id,
            remote_id,
            client_writer,
            max_payload,
            state: Arc::new(Mutex::new(ServiceState::default())),
            wake: Arc::new(Notify::new()),
            transport_writer: None,
        }
    }

    /// Writes a packet to the client.
    async fn write(&self, packet: Vec<u8>) {
        let _ = self.client_writer.lock().await.write_all(&packet).await;
    }

    /// The local id of the service.
    pub fn local_id(&self) -> u32 {
        self.local_id
    }

    /// The remote id of the service.
    pub fn remote_id(&self) -> u32 {
        self.remote_id
    }

    /// Whether the service has ended.
    pub async fn is_ended(&self) -> bool {
        self.state.lock().await.ended
    }

    /// Handles a packet.
    ///
    /// Ported from `Service.handle()`.
    pub async fn handle(&mut self, packet: Packet) {
        let result = match packet.command {
            packet::A_OPEN => self.handle_open(packet).await,
            packet::A_OKAY => self.handle_okay(packet).await,
            packet::A_WRTE => self.handle_write(packet).await,
            packet::A_CLSE => self.handle_close(packet).await,
            command => Err(Error::Device(format!("Unexpected packet {}", command))),
        };
        if let Err(err) = result {
            crate::debug!("Service error: {}", err);
            self.end().await;
        }
    }

    /// Handles `A_OPEN`: connects to the device and forwards the service name.
    ///
    /// Ported from `Service._handleOpenPacket()`.
    async fn handle_open(&mut self, packet: Packet) -> Result<()> {
        crate::debug!("I:A_OPEN {}", packet);

        let device = self.client.get_device(&self.serial);
        let mut transport = device.transport().await?;
        if self.state.lock().await.ended {
            return Err(Error::LateTransport);
        }

        let data = packet.data.unwrap_or_default();
        // Discard the null byte at the end.
        let service_name =
            String::from_utf8_lossy(&data[..data.len().saturating_sub(1)]).into_owned();
        crate::command::send(&mut transport, &service_name).await?;

        match crate::parser::read_reply(transport.parser()).await? {
            crate::protocol::Reply::Okay => {
                crate::debug!("O:A_OKAY");
                self.write(Packet::assemble(
                    packet::A_OKAY,
                    self.local_id,
                    self.remote_id,
                    None,
                ))
                .await;
                self.state.lock().await.opened = true;
            }
            reply => {
                self.end().await;
                return transport
                    .parser()
                    .unexpected(reply.to_string(), "OKAY or FAIL");
            }
        }

        // Split the transport: the read half pumps data to the client, the
        // write half is used for data coming from the client.
        let (reader, writer) = transport.into_split();
        self.transport_writer = Some(Arc::new(Mutex::new(writer)));

        spawn_pump(
            reader,
            Arc::clone(&self.state),
            Arc::clone(&self.wake),
            Arc::clone(&self.client_writer),
            self.max_payload,
            self.local_id,
            self.remote_id,
        );

        Ok(())
    }

    /// Handles `A_OKAY`: the client is ready for more data.
    ///
    /// Ported from `Service._handleOkayPacket()`.
    async fn handle_okay(&mut self, packet: Packet) -> Result<()> {
        crate::debug!("I:A_OKAY {}", packet);
        if self.state.lock().await.ended {
            return Ok(());
        }
        if self.transport_writer.is_none() {
            return Err(Error::PrematurePacket);
        }
        self.ack();
        Ok(())
    }

    /// Handles `A_WRTE`: writes the payload to the device.
    ///
    /// Ported from `Service._handleWritePacket()`.
    async fn handle_write(&mut self, packet: Packet) -> Result<()> {
        crate::debug!("I:A_WRTE {}", packet);
        if self.state.lock().await.ended {
            return Ok(());
        }
        let writer = match self.transport_writer.as_ref() {
            Some(writer) => writer.clone(),
            None => return Err(Error::PrematurePacket),
        };
        if let Some(data) = &packet.data {
            writer
                .lock()
                .await
                .write_all(data)
                .await
                .map_err(Error::from)?;
        }
        crate::debug!("O:A_OKAY");
        self.write(Packet::assemble(
            packet::A_OKAY,
            self.local_id,
            self.remote_id,
            None,
        ))
        .await;
        Ok(())
    }

    /// Handles `A_CLSE`: the client closed the service.
    ///
    /// Ported from `Service._handleClosePacket()`.
    async fn handle_close(&mut self, packet: Packet) -> Result<()> {
        crate::debug!("I:A_CLSE {}", packet);
        if self.state.lock().await.ended {
            return Ok(());
        }
        if self.transport_writer.is_none() {
            return Err(Error::PrematurePacket);
        }
        self.end().await;
        Ok(())
    }

    /// Ends the service, closing the transport connection.
    ///
    /// Ported from `Service.end()`.
    pub async fn end(&mut self) {
        // Closing the writer ends the transport connection.
        self.transport_writer = None;
        if self.state.lock().await.ended {
            return;
        }
        crate::debug!("O:A_CLSE");
        // Zero can only mean a failed open.
        let local_id = if self.state.lock().await.opened {
            self.local_id
        } else {
            0
        };
        self.write(Packet::assemble(
            packet::A_CLSE,
            local_id,
            self.remote_id,
            None,
        ))
        .await;
        self.state.lock().await.ended = true;
        self.wake.notify_waiters();
    }

    /// Acknowledges the last packet, allowing more data to be sent.
    ///
    /// Called when an `A_OKAY` arrives, mirroring `_tryPush()` being called from
    /// `_handleOkayPacket()` in the TypeScript original.
    pub fn ack(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            if state.ended {
                return;
            }
            state.need_ack = false;
        }
        self.wake.notify_waiters();
    }
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service")
            .field("local_id", &self.local_id)
            .field("remote_id", &self.remote_id)
            .finish_non_exhaustive()
    }
}

/// Spawns the task that pumps data from the device to the client.
fn spawn_pump(
    mut reader: ReadHalf<TcpStream>,
    state: Arc<Mutex<ServiceState>>,
    wake: Arc<Notify>,
    client_writer: Arc<Mutex<WriteHalf<TcpStream>>>,
    max_payload: u32,
    local_id: u32,
    remote_id: u32,
) {
    tokio::spawn(async move {
        let mut chunk = vec![0u8; 4096];
        loop {
            {
                let state = state.lock().await;
                if state.ended {
                    break;
                }
                if state.need_ack {
                    drop(state);
                    // Wait for the client to acknowledge the last packet.
                    wake.notified().await;
                    continue;
                }
            }

            let read = tokio::select! {
                read = reader.read(&mut chunk) => read,
                _ = wake.notified() => continue,
            };
            let read = match read {
                Ok(read) => read,
                Err(err) => {
                    crate::debug!("Transport error: {}", err);
                    break;
                }
            };
            if read == 0 {
                break;
            }
            chunk.truncate(read);

            crate::debug!("O:A_WRTE");
            let data = &chunk[..chunk.len().min(max_payload as usize)];
            let _ = client_writer
                .lock()
                .await
                .write_all(&Packet::assemble(
                    packet::A_WRTE,
                    local_id,
                    remote_id,
                    Some(data),
                ))
                .await;
            state.lock().await.need_ack = true;
        }
    });
}
