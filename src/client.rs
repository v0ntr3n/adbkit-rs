//! The ADB client.
//!
//! Ported from `src/adb/client.ts`.

use tokio::process;

use crate::command;
use crate::connection::Connection;
use crate::error::{Error, Result};
use crate::tcpusb::TcpUsbServer;
use crate::tracker::TrackerHandle;
use crate::types::{ClientOptions, Device, DeviceWithPath, SocketOptions};
use crate::DeviceClient;

/// A client for the ADB server.
///
/// ```no_run
/// # async fn example() -> Result<(), adbkit::Error> {
/// use adbkit::Adb;
///
/// let client = Adb::create_client(Default::default());
/// for device in client.list_devices().await? {
///     println!("{} is {}", device.id, device.r#type);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    options: ClientOptions,
    host: String,
    port: u16,
    bin: String,
    timeout: u64,
}

impl Client {
    /// Creates a client with the given options.
    pub fn new(options: ClientOptions) -> Self {
        let host = options
            .host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = options.port.unwrap_or(5037);
        let bin = options.bin.clone();
        let timeout = options.timeout;
        Client {
            options,
            host,
            port,
            bin,
            timeout,
        }
    }

    /// The options this client was created with.
    pub fn options(&self) -> &ClientOptions {
        &self.options
    }

    /// The host of the ADB server.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port the ADB server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The path to the `adb` binary.
    pub fn bin(&self) -> &str {
        &self.bin
    }

    /// The socket timeout in milliseconds. `0` means no timeout.
    pub fn timeout(&self) -> u64 {
        self.timeout
    }

    /// Opens a new connection to the ADB server.
    ///
    /// Connections are not reused: the TypeScript implementation opens a new
    /// one for every logical group of commands, and so does this port.
    pub async fn connection(&self) -> Result<Connection<tokio::net::TcpStream>> {
        Connection::connect(self).await
    }

    /// Starts the ADB server by running `adb start-server`.
    ///
    /// This is the only case where this library falls back to the `adb`
    /// binary.
    pub async fn start_server(&self) -> Result<()> {
        let port = self.port;
        let mut command = process::Command::new(&self.bin);
        if port != 0 {
            command.args(["-P", &port.to_string(), "start-server"]);
        } else {
            command.arg("start-server");
        }
        crate::debug!(
            "Starting ADB server via '{} {}'",
            self.bin,
            if port != 0 {
                format!("-P {} start-server", port)
            } else {
                "start-server".to_string()
            }
        );
        crate::debug!("CLI: {} {:?}", self.bin, command.as_std().get_args());
        let output = command.output().await?;
        if !output.status.success() {
            return Err(Error::Device(format!(
                "Unable to start the ADB server: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    /// Queries the ADB server for its version.
    pub async fn version(&self) -> Result<u32> {
        let mut connection = self.connection().await?;
        command::host::version(&mut connection).await
    }

    /// Connects to the given device, which must have its ADB daemon running in
    /// tcp mode. Same as `adb connect <host>:<port>`.
    pub async fn connect(&self, host: &str, port: Option<u16>) -> Result<String> {
        let (host, port) = split_host_port(host, port);
        let mut connection = self.connection().await?;
        command::host::connect(&mut connection, &host, port).await
    }

    /// Disconnects from the given device. Same as `adb disconnect <host>:<port>`.
    ///
    /// Returns the disconnected device ID.
    pub async fn disconnect(&self, host: &str, port: Option<u16>) -> Result<String> {
        let (host, port) = split_host_port(host, port);
        let mut connection = self.connection().await?;
        command::host::disconnect(&mut connection, &host, port).await
    }

    /// Gets the list of currently connected devices and emulators.
    pub async fn list_devices(&self) -> Result<Vec<Device>> {
        let mut connection = self.connection().await?;
        command::host::devices(&mut connection).await
    }

    /// Like [`Client::list_devices`], but includes the "path" of every device.
    pub async fn list_devices_with_paths(&self) -> Result<Vec<DeviceWithPath>> {
        let mut connection = self.connection().await?;
        command::host::devices_with_paths(&mut connection).await
    }

    /// Gets a device tracker.
    ///
    /// Note that as the tracker keeps a connection open, you must drop it (or
    /// call [`TrackerHandle::end`]) if you wish to stop tracking devices.
    pub async fn track_devices(&self) -> Result<TrackerHandle> {
        let connection = self.connection().await?;
        command::host::track_devices(connection).await
    }

    /// Kills the ADB server.
    ///
    /// Note that the next connection will attempt to start the server again
    /// when it's unable to connect.
    pub async fn kill(&self) -> Result<bool> {
        let mut connection = self.connection().await?;
        command::host::kill(&mut connection).await
    }

    /// Returns a client for the device with the given serial number.
    pub fn get_device(&self, serial: impl Into<String>) -> DeviceClient {
        DeviceClient::new(self.clone(), serial.into())
    }

    /// Creates a TCP-USB bridge: an ADB server, implemented by this crate,
    /// that provides a USB device over TCP using a translating proxy.
    pub fn create_tcp_usb_bridge(
        &self,
        serial: impl Into<String>,
        options: SocketOptions,
    ) -> TcpUsbServer {
        TcpUsbServer::new(self.clone(), serial.into(), options)
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("bin", &self.bin)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

/// Splits a `host:port` pair, honouring an explicit port argument.
fn split_host_port(host: &str, port: Option<u16>) -> (String, u16) {
    match host.split_once(':') {
        Some((host, port_string)) => {
            let port = port_string.parse::<u16>().ok();
            (host.to_string(), port.or(port).unwrap_or(5555))
        }
        None => (host.to_string(), port.unwrap_or(5555)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let client = Client::new(ClientOptions::default());
        assert_eq!(client.host(), "127.0.0.1");
        assert_eq!(client.port(), 5037);
        assert_eq!(client.bin(), "adb");
        assert_eq!(client.timeout(), 0);
    }

    #[test]
    fn test_custom_options() {
        let client = Client::new(ClientOptions {
            host: Some("example.com".to_string()),
            port: Some(5038),
            bin: "/usr/local/bin/adb".to_string(),
            timeout: 5000,
        });
        assert_eq!(client.host(), "example.com");
        assert_eq!(client.port(), 5038);
        assert_eq!(client.bin(), "/usr/local/bin/adb");
        assert_eq!(client.timeout(), 5000);
    }

    #[test]
    fn test_split_host_port() {
        assert_eq!(
            split_host_port("192.168.2.2:5555", None),
            ("192.168.2.2".to_string(), 5555)
        );
        assert_eq!(
            split_host_port("192.168.2.2", Some(5556)),
            ("192.168.2.2".to_string(), 5556)
        );
        assert_eq!(
            split_host_port("192.168.2.2:1111", Some(5556)),
            ("192.168.2.2".to_string(), 1111)
        );
    }
}
