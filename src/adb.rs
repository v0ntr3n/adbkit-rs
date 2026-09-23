//! The `Adb` facade.
//!
//! Ported from `src/adb.ts`.

use crate::client::Client;
use crate::types::ClientOptions;

/// Options for [`Adb::create_client`].
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The host of the ADB server. Defaults to `127.0.0.1`, or the `ADB_HOST`
    /// environment variable.
    pub host: Option<String>,
    /// The port where the ADB server is listening. Defaults to `5037`, or the
    /// `ADB_PORT` environment variable.
    pub port: Option<u16>,
    /// The path to the `adb` binary, used for starting the server locally if
    /// the initial connection fails. Defaults to `adb`.
    pub bin: Option<String>,
    /// The socket timeout in milliseconds. Defaults to `0` which means no
    /// timeout.
    pub timeout: Option<u64>,
}

/// The adbkit entry point.
pub struct Adb;

impl Adb {
    /// Creates a client instance with the provided options.
    ///
    /// Note that this will not automatically establish a connection, it will
    /// only be done when necessary.
    pub fn create_client(options: Options) -> Client {
        let options = ClientOptions {
            bin: options.bin.unwrap_or_else(|| "adb".to_string()),
            host: options.host.or_else(|| std::env::var("ADB_HOST").ok()),
            port: options.port.or_else(|| {
                std::env::var("ADB_PORT")
                    .ok()
                    .and_then(|port| port.trim().parse::<u16>().ok())
            }),
            timeout: options.timeout.unwrap_or(0),
        };
        Client::new(options)
    }

    /// Creates a client instance with the default options.
    pub fn create_client_default() -> Client {
        Client::new(ClientOptions::default())
    }
}

/// Utilities for working with streams and public keys.
pub use crate::util::Util;
/// Android key codes.
///
/// Re-exported for convenience; see [`crate::KeyCodes`].
pub use crate::KeyCodes as Keycode;
