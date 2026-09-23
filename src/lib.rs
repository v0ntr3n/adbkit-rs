//! # adbkit
//!
//! A pure Rust client for the [Android Debug Bridge][adb] server, ported from
//! the TypeScript [`@devicefarmer/adbkit`][adbkit] library.
//!
//! [adb]: https://developer.android.com/studio/command-line/adb
//! [adbkit]: https://github.com/devicefarmer/adbkit
//!
//! ```no_run
//! use adbkit::Adb;
//!
//! # async fn example() -> Result<(), adbkit::Error> {
//! let client = Adb::create_client(Default::default());
//! for device in client.list_devices().await? {
//!     println!("{} is {}", device.id, device.r#type);
//! }
//! # Ok(())
//! # }
//! ```

/// Logs a debug message under the `adb` namespace.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)+) => { log::debug!(target: "adb", $($arg)+) };
}

/// Logs an informational message under the `adb` namespace.
#[macro_export]
macro_rules! info {
    ($($arg:tt)+) => { log::info!(target: "adb", $($arg)+) };
}

/// Logs a warning under the `adb` namespace.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)+) => { log::warn!(target: "adb", $($arg)+) };
}

/// Logs an error under the `adb` namespace.
#[macro_export]
macro_rules! error {
    ($($arg:tt)+) => { log::error!(target: "adb", $($arg)+) };
}

/// Initializes logging from the `DEBUG` environment variable (as used by the
/// original `debug` dependency), falling back to `RUST_LOG`.
///
/// ```no_run
/// adbkit::init_logging();
/// ```
pub fn init_logging() {
    use std::io::Write;

    let mut builder = env_logger::Builder::from_default_env();
    if let Ok(debug) = std::env::var("DEBUG") {
        // The npm `debug` package understands patterns such as `adb:*` and
        // `adb:connection`. Translate the common cases.
        let spec = debug
            .split(',')
            .map(|part| {
                let part = part.trim();
                if part == "adb" || part == "adb:*" {
                    "adbkit=debug".to_string()
                } else if let Some(rest) = part.strip_prefix("adb:") {
                    if rest == "*" {
                        "adbkit=debug".to_string()
                    } else {
                        format!("adbkit::{}={}", rest.replace(':', "::"), "debug")
                    }
                } else {
                    part.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        builder.parse_filters(&spec);
    }
    let _ = builder
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .try_init();
}

pub mod adb;
pub mod auth;
pub mod command;
pub mod framebuffer;
pub mod logcat;
pub mod monkey;
pub mod proc;
pub mod sync;
pub mod tcpusb;

mod client;
mod connection;
mod device_client;
pub mod error;
pub mod jdwptracker;
mod keycode;
pub mod linetransform;
pub mod parser;
pub mod protocol;
mod tracker;
pub mod types;
pub mod util;

pub use adb::Adb;
pub use auth::{Auth, ExtendedPublicKey};
pub use client::Client;
pub use connection::{Connection, RawStream};
pub use device_client::{DeviceClient, PushSource, ScreenshotStream};

pub use error::{Error, Result};
pub use framebuffer::{parse_header, FramebufferStream, RgbTransform};
pub use jdwptracker::{JdwpTracker, JdwpTrackerChangeSet, JdwpTrackerEvent};
pub use keycode::KeyCodes;
pub use parser::Parser;
pub use proc::stat::ProcStat;
pub use protocol::{parse_reply, Protocol, Reply};
pub use sync::{temp, Entry, PullOptions, PullTransfer, PushOptions, Stats, Sync};
pub use tracker::{Tracker, TrackerEvent, TrackerHandle};
pub use types::{
    ClientOptions, ColorFormat, CpuStats, Device, DeviceType, DeviceWithPath, Extra, ExtraValue,
    Extras, Features, Forward, FramebufferMeta, Loads, Properties, Reverse, SocketOptions,
    StartActivityOptions, StartServiceOptions, TrackerChangeSet,
};
pub use util::Util;

/// The stream returned by [`DeviceClient::shell`], `openLocal()`, `openTcp()`
/// and `openLog()`.
pub type ShellStream = RawStream<tokio::net::TcpStream>;
