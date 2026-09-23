//! A single connection to the ADB server implemented by this crate.
//!
//! Ported from `src/adb/tcpusb/socket.ts`.

use std::sync::Arc;

use getrandom::getrandom;
use tokio::io::{AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::auth::Auth;
use crate::client::Client;
use crate::error::{Error, Result};
use crate::tcpusb::packet::{self, Packet};
use crate::tcpusb::packetreader::PacketReader;
use crate::tcpusb::rollingcounter::RollingCounter;
use crate::tcpusb::service::Service;
use crate::tcpusb::servicemap::ServiceMap;
use crate::types::SocketOptions;

/// The authentication token length, in bytes.
pub const TOKEN_LENGTH: usize = 20;
/// `AUTH_TOKEN`
pub const AUTH_TOKEN: u32 = 1;
/// `AUTH_SIGNATURE`
pub const AUTH_SIGNATURE: u32 = 2;
/// `AUTH_RSAPUBLICKEY`
pub const AUTH_RSAPUBLICKEY: u32 = 3;

/// A connection from an ADB client.
///
/// Ported from the `Socket` class of `src/adb/tcpusb/socket.ts`.
pub struct Socket {
    writer: Arc<Mutex<WriteHalf<TcpStream>>>,
    client: Arc<Client>,
    serial: String,
    options: SocketOptions,
    ended: bool,
    authorized: bool,
    version: u32,
    max_payload: u32,
    sync_token: RollingCounter,
    remote_id: RollingCounter,
    services: ServiceMap,
    token: Option<Vec<u8>>,
    signature: Option<Vec<u8>>,
}

impl Socket {
    /// The maximum payload size of this connection.
    pub fn max_payload(&self) -> u32 {
        self.max_payload
    }
}

impl Socket {
    /// Whether this connection has ended.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Writes a packet to the client.
    async fn write(&self, packet: Vec<u8>) {
        if self.ended {
            return;
        }
        let _ = self.writer.lock().await.write_all(&packet).await;
    }

    /// Ends the connection, closing every service.
    ///
    /// Ported from `Socket.end()`.
    pub async fn end(&mut self) {
        if self.ended {
            return;
        }
        // End services first so that they can send a final payload before FIN.
        self.services.end().await;
        self.ended = true;
        let _ = self.writer.lock().await.shutdown().await;
    }

    /// Handles a single packet.
    ///
    /// Ported from `Socket._handle()`.
    async fn handle_packet(&mut self, packet: Packet) -> Result<()> {
        match packet.command {
            packet::A_SYNC => {
                crate::debug!("I:A_SYNC");
                let id = self.sync_token.next();
                crate::debug!("O:A_SYNC");
                self.write(Packet::assemble(packet::A_SYNC, 1, id, None))
                    .await;
            }
            packet::A_CNXN => self.handle_connection(packet).await?,
            packet::A_AUTH => self.handle_auth(packet).await?,
            packet::A_OPEN => self.handle_open(packet).await?,
            packet::A_OKAY | packet::A_WRTE | packet::A_CLSE => {
                self.forward_service_packet(packet).await?
            }
            command => return Err(Error::Device(format!("Unknown command {}", command))),
        }
        Ok(())
    }

    /// Handles `A_CNXN`: sends the authentication challenge.
    ///
    /// Ported from `Socket._handleConnectionPacket()`.
    async fn handle_connection(&mut self, packet: Packet) -> Result<()> {
        crate::debug!("I:A_CNXN {}", packet);
        self.version = Packet::swap32(packet.arg0);
        self.max_payload = packet.arg1.min(0xffff);

        let mut token = vec![0u8; TOKEN_LENGTH];
        getrandom(&mut token)
            .map_err(|err| Error::Device(format!("Unable to get randomness: {}", err)))?;
        self.token = Some(token.clone());
        crate::debug!("Created challenge");

        crate::debug!("O:A_AUTH");
        self.write(Packet::assemble(
            packet::A_AUTH,
            AUTH_TOKEN,
            0,
            Some(&token),
        ))
        .await;
        Ok(())
    }

    /// Handles `A_AUTH`.
    ///
    /// Ported from `Socket._handleAuthPacket()`.
    async fn handle_auth(&mut self, packet: Packet) -> Result<()> {
        crate::debug!("I:A_AUTH {}", packet);
        match packet.arg0 {
            AUTH_SIGNATURE => {
                // Store the first signature, ignore the rest.
                if packet.data.is_some() {
                    crate::debug!("Received signature");
                }
                if self.signature.is_none() {
                    self.signature = packet.data.clone();
                }

                let token = self.token.clone().unwrap_or_default();
                let signature = self.signature.clone().unwrap_or_default();
                for key in &self.options.known_public_keys {
                    // If the signature matches one of the known public keys, we
                    // can safely accept the connection.
                    if key.verify(&token, &signature) {
                        return self.accept_connection().await;
                    }
                }

                crate::debug!("O:A_AUTH");
                self.write(Packet::assemble(
                    packet::A_AUTH,
                    AUTH_TOKEN,
                    0,
                    Some(&token),
                ))
                .await;
            }
            AUTH_RSAPUBLICKEY => {
                let signature = match self.signature.clone() {
                    Some(signature) => signature,
                    None => {
                        return Err(Error::Auth("Public key sent before signature".to_string()))
                    }
                };
                let data = packet.data.unwrap_or_default();
                if data.len() < 2 {
                    return Err(Error::Auth("Empty RSA public key".to_string()));
                }
                crate::debug!("Received RSA public key");
                // Discard the null byte at the end.
                let text = String::from_utf8_lossy(&data[..data.len() - 1]).into_owned();
                let key = Auth::parse_public_key(&text)?;

                let token = self.token.clone().unwrap_or_default();
                if !key.verify(&token, &signature) {
                    crate::debug!("Signature mismatch");
                    return Err(Error::Auth("Signature mismatch".to_string()));
                }
                crate::debug!("Signature verified");

                if let Some(auth) = self.options.auth.as_ref() {
                    if !auth(key) {
                        crate::debug!("Connection rejected by user-defined auth handler");
                        return Err(Error::Auth("Rejected by user-defined handler".to_string()));
                    }
                }

                return self.accept_connection().await;
            }
            other => {
                return Err(Error::Device(format!(
                    "Unknown authentication method {}",
                    other
                )))
            }
        }
        Ok(())
    }

    /// Marks the connection as authorized and sends `A_CNXN`.
    ///
    /// Ported from `Socket._acceptConnection()`.
    async fn accept_connection(&mut self) -> Result<()> {
        let id = self.device_id().await?;
        self.authorized = true;
        crate::debug!("O:A_CNXN");
        self.write(Packet::assemble(
            packet::A_CNXN,
            Packet::swap32(self.version),
            self.max_payload,
            Some(id.as_bytes()),
        ))
        .await;
        Ok(())
    }

    /// Builds the device id from the device properties.
    ///
    /// Ported from `Socket._deviceId()`.
    async fn device_id(&self) -> Result<String> {
        crate::debug!("Loading device properties to form a standard device ID");
        let properties = self
            .client
            .get_device(&self.serial)
            .get_properties()
            .await?;
        let id = ["ro.product.name", "ro.product.model", "ro.product.device"]
            .iter()
            .map(|prop| {
                format!(
                    "{}={};",
                    prop,
                    properties.get(*prop).cloned().unwrap_or_default()
                )
            })
            .collect::<String>();
        Ok(format!("device::{}\x00", id))
    }

    /// Handles `A_OPEN`: starts a new service.
    ///
    /// Ported from `Socket._handleOpenPacket()`.
    async fn handle_open(&mut self, packet: Packet) -> Result<()> {
        if !self.authorized {
            return Err(Error::Unauthorized);
        }
        let remote_id = packet.arg0;
        let local_id = self.remote_id.next();
        let data = packet.data.clone().unwrap_or_default();
        if data.len() < 2 {
            return Err(Error::Device("Empty service name".to_string()));
        }
        let name = String::from_utf8_lossy(&data[..data.len() - 1]).into_owned();
        crate::debug!("Calling {}", name);

        let service = Service::new(
            self.client.clone(),
            self.serial.clone(),
            local_id,
            remote_id,
            Arc::clone(&self.writer),
            self.max_payload,
        );
        self.services.insert(local_id, service)?;
        crate::debug!("Handling {} services simultaneously", self.services.count());

        let packet_copy = packet;
        if let Some(service) = self.services.get_mut(local_id) {
            service.handle(packet_copy).await;
        }
        crate::debug!("Handling {} services simultaneously", self.services.count());
        Ok(())
    }

    /// Forwards a packet to the service it belongs to.
    ///
    /// Ported from `Socket._forwardServicePacket()`.
    async fn forward_service_packet(&mut self, packet: Packet) -> Result<()> {
        if !self.authorized {
            return Err(Error::Unauthorized);
        }
        let local_id = packet.arg1;
        match self.services.get_mut(local_id) {
            Some(service) => {
                service.handle(packet).await;
                Ok(())
            }
            None => {
                crate::debug!("Received a packet to a service that may have been closed already");
                Ok(())
            }
        }
    }
}

/// A connection to the ADB server implemented by this crate, with its reader
/// half kept separate so that no lock is held while waiting for data.
pub struct SocketConnection {
    handle: Arc<Mutex<Socket>>,
    reader: ReadHalf<TcpStream>,
}

impl SocketConnection {
    /// Wraps a new connection.
    pub fn new(
        client: Arc<Client>,
        serial: String,
        stream: TcpStream,
        options: SocketOptions,
    ) -> Self {
        stream.set_nodelay(true).ok();
        let (reader, writer) = tokio::io::split(stream);
        let writer = Arc::new(Mutex::new(writer));
        let handle = Arc::new(Mutex::new(Socket {
            writer: Arc::clone(&writer),
            client,
            serial,
            options,
            ended: false,
            authorized: false,
            version: 1,
            max_payload: 4096,
            sync_token: RollingCounter::default(),
            remote_id: RollingCounter::default(),
            services: ServiceMap::new(),
            token: None,
            signature: None,
        }));
        SocketConnection { handle, reader }
    }

    /// Runs the connection until the client disconnects.
    pub async fn run(mut self) {
        let handle = self.handle.clone();
        let mut reader = PacketReader::new(&mut self.reader);

        while let Some(packet) = reader.next().await {
            let packet = match packet {
                Ok(packet) => packet,
                Err(err) => {
                    crate::debug!("PacketReader error: {}", err);
                    let mut socket = handle.lock().await;
                    socket.end().await;
                    return;
                }
            };

            let mut socket = handle.lock().await;
            if socket.is_ended() {
                return;
            }
            if let Err(err) = socket.handle_packet(packet).await {
                crate::debug!("Socket error: {}", err);
                socket.end().await;
                return;
            }
        }

        let mut socket = handle.lock().await;
        socket.end().await;
    }

    /// Ends the connection.
    pub async fn end(&self) {
        let mut socket = self.handle.lock().await;
        socket.end().await;
    }
}
