//! Plain data types shared by the client.
//!
//! These correspond to the interfaces in the TypeScript sources: `src/Device.ts`,
//! `src/DeviceWithPath.ts`, `src/Forward.ts`, `src/Reverse.ts`,
//! `src/Properties.ts`, `src/Features.ts`, `src/FramebufferMeta.ts`,
//! `src/ClientOptions.ts`, `src/SocketOptions.ts`, `src/Callback.ts`,
//! `src/StartActivityOptions.ts`, `src/StartServiceOptions.ts`,
//! `src/TrackerChangeSet.ts`, `src/ExtendedPublicKey.ts` and `src/CpuStats.ts`.

use std::collections::HashMap;

use crate::auth::ExtendedPublicKey;

impl std::fmt::Debug for SocketOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocketOptions")
            .field("auth", &self.auth.as_ref().map(|_| "<callback>"))
            .field("known_public_keys", &self.known_public_keys.len())
            .finish()
    }
}

/// A device, as returned by `Client::listDevices()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The ID of the device. For real devices, this is usually the USB
    /// identifier.
    pub id: String,
    /// The device type.
    pub r#type: DeviceType,
}

impl Device {
    /// Creates a new device.
    pub fn new<I: Into<String>>(id: I, r#type: DeviceType) -> Self {
        Device {
            id: id.into(),
            r#type,
        }
    }
}

/// The type of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// An emulator.
    Emulator,
    /// A real device.
    Device,
    /// An offline device, e.g. during boot or in low-battery conditions.
    Offline,
    /// A device that hasn't been authorized yet.
    Unauthorized,
    /// Anything else.
    Unknown,
}

impl DeviceType {
    /// Parses the type as reported by the ADB server.
    pub fn parse(value: &str) -> DeviceType {
        match value {
            "emulator" => DeviceType::Emulator,
            "device" => DeviceType::Device,
            "offline" => DeviceType::Offline,
            "unauthorized" => DeviceType::Unauthorized,
            _ => DeviceType::Unknown,
        }
    }

    /// The type as reported by the ADB server.
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceType::Emulator => "emulator",
            DeviceType::Device => "device",
            DeviceType::Offline => "offline",
            DeviceType::Unauthorized => "unauthorized",
            DeviceType::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for DeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A device with its "path", as returned by `Client::listDevicesWithPaths()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceWithPath {
    /// See [`Device::id`].
    pub id: String,
    /// See [`Device::type`].
    pub r#type: DeviceType,
    /// The device path, e.g. `usb:FD120000` for real devices.
    pub path: String,
    /// The product name.
    pub product: String,
    /// The model.
    pub model: String,
    /// The device name.
    pub device: String,
    /// The transport id.
    pub transport_id: String,
}

/// A forwarded connection, as returned by `DeviceClient::listForwards()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    /// The device serial.
    pub serial: String,
    /// The local endpoint on the ADB host.
    pub local: String,
    /// The remote endpoint on the device.
    pub remote: String,
}

/// A reversed connection, as returned by `DeviceClient::listReverses()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reverse {
    /// The remote endpoint on the device.
    pub remote: String,
    /// The local endpoint on the ADB host.
    pub local: String,
}

/// Device properties, e.g. as returned by `DeviceClient::getProperties()`.
pub type Properties = HashMap<String, String>;

/// Device features, e.g. as returned by `DeviceClient::getFeatures()`.
pub type Features = HashMap<String, bool>;

/// The color format of a framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFormat {
    /// Blue, green, red.
    Bgr,
    /// Blue, green, red, alpha.
    Bgra,
    /// Red, green, blue.
    Rgb,
    /// Red, green, blue, alpha.
    Rgba,
}

impl ColorFormat {
    /// The format as a string, as used by the `gm` command.
    pub fn as_str(&self) -> &'static str {
        match self {
            ColorFormat::Bgr => "bgr",
            ColorFormat::Bgra => "bgra",
            ColorFormat::Rgb => "rgb",
            ColorFormat::Rgba => "rgba",
        }
    }
}

impl std::fmt::Display for ColorFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Metadata describing a raw framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramebufferMeta {
    /// The framebuffer version. Useful for patching backwards-compatibility
    /// issues.
    pub version: u32,
    /// The framebuffer format for convenience. One of `bgr`, `bgra`, `rgb`
    /// or `rgba`.
    pub format: ColorFormat,
    /// The horizontal resolution of the framebuffer.
    pub width: u32,
    /// The vertical resolution of the framebuffer.
    pub height: u32,
    /// Bits per pixel (i.e. color depth).
    pub bpp: u32,
    /// The raw byte size of the framebuffer.
    pub size: u32,
    /// The bit offset of the red color in a pixel.
    pub red_offset: u32,
    /// The bit length of the red color in a pixel.
    pub red_length: u32,
    /// The bit offset of the blue color in a pixel.
    pub blue_offset: u32,
    /// The bit length of the blue color in a pixel.
    pub blue_length: u32,
    /// The bit offset of the green color in a pixel.
    pub green_offset: u32,
    /// The bit length of the green color in a pixel.
    pub green_length: u32,
    /// The bit offset of alpha in a pixel.
    pub alpha_offset: u32,
    /// The bit length of alpha in a pixel. `0` when not available.
    pub alpha_length: u32,
}

/// Options for creating a client.
///
/// Ported from `src/ClientOptions.ts`, which extends Node's
/// `Net.connect` options.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// The host of the ADB server. Defaults to `127.0.0.1`.
    pub host: Option<String>,
    /// The port where the ADB server is listening. Defaults to `5037`.
    pub port: Option<u16>,
    /// The path to the `adb` binary, used for starting the server locally if
    /// the initial connection fails. Defaults to `adb`.
    pub bin: String,
    /// The socket timeout in milliseconds. Defaults to `0`, which means no
    /// timeout.
    pub timeout: u64,
}

impl Default for ClientOptions {
    fn default() -> Self {
        ClientOptions {
            host: None,
            port: None,
            bin: "adb".to_string(),
            timeout: 0,
        }
    }
}

/// Options for a TCP-USB bridge (an ADB server implemented by this crate).
///
/// Ported from `src/SocketOptions.ts`.
#[derive(Clone, Default)]
pub struct SocketOptions {
    /// Called with the public key of a connecting client. Resolve with `true`
    /// (or nothing) to accept the connection, or `false` to reject it.
    pub auth: Option<std::sync::Arc<dyn Fn(ExtendedPublicKey) -> bool + Send + Sync>>,
    /// Public keys that are accepted without calling `auth`.
    pub known_public_keys: Vec<ExtendedPublicKey>,
}

/// A callback used by the Node.js API. Kept for documentation purposes; the
/// Rust API is entirely `async`.
pub type Callback<T> = Box<dyn FnOnce(Option<Error>, Option<T>) + Send>;

/// One of the extra values accepted by `am start`/`am startservice`.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtraValue {
    /// A string extra.
    String(String),
    /// A boolean extra.
    Bool(bool),
    /// An integer extra.
    Int(i64),
    /// A long extra.
    Long(i64),
    /// A float extra.
    Float(f64),
    /// A null extra (`sn`).
    Null,
    /// A URI extra.
    Uri(String),
    /// A component extra.
    Component(String),
}

/// An extra, in the "long" syntax.
///
/// Ported from `src/StartServiceOptions.ts`'s `Extra`.
#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    /// The key name.
    pub key: String,
    /// The value. Unused when `r#type` is `null`.
    pub value: ExtraValue,
}

impl Extra {
    /// Creates a new extra.
    pub fn new<K: Into<String>>(key: K, value: ExtraValue) -> Self {
        Extra {
            key: key.into(),
            value,
        }
    }
}

/// Options for `DeviceClient::startService()`.
///
/// Ported from `src/StartServiceOptions.ts`.
#[derive(Debug, Clone, Default)]
pub struct StartServiceOptions {
    /// The user to run as. Not set by default.
    pub user: Option<u32>,
    /// The action.
    pub action: Option<String>,
    /// The data URI, if any.
    pub data: Option<String>,
    /// The mime type.
    pub mime_type: Option<String>,
    /// The category. For multiple categories, pass a `Vec`.
    pub category: Option<Vec<String>>,
    /// The component.
    pub component: Option<String>,
    /// Numeric flags.
    pub flags: Option<Vec<u32>>,
    /// Any extra data.
    pub extras: Option<Extras>,
}

/// Extra data, either as a list of [`Extra`] or as a key/value map.
#[derive(Debug, Clone, PartialEq)]
pub enum Extras {
    /// Long syntax.
    List(Vec<Extra>),
    /// Short syntax.
    Map(Vec<(String, ExtraValue)>),
}

/// Options for `DeviceClient::startActivity()`.
///
/// Ported from `src/StartActivityOptions.ts`.
#[derive(Debug, Clone, Default)]
pub struct StartActivityOptions {
    /// Set to `true` to enable debugging.
    pub debug: bool,
    /// Set to `true` to wait for the activity to launch.
    pub wait: bool,
    /// The service options.
    #[doc(hidden)]
    pub service: StartServiceOptions,
}

/// All changes reported by the ADB server in a single run.
///
/// Ported from `src/TrackerChangeSet.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackerChangeSet {
    /// Devices that were removed. Empty if none.
    pub removed: Vec<Device>,
    /// Devices whose type changed. Empty if none.
    pub changed: Vec<Device>,
    /// Devices that were added. Empty if none.
    pub added: Vec<Device>,
}

/// CPU statistics, as tracked by `DeviceClient::openProcStat()`.
///
/// Ported from `src/CpuStats.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuStats {
    /// Percentage (0-100) of ticks spent on user programs.
    pub user: f64,
    /// Percentage (0-100) of ticks spent on `nice`d user programs.
    pub nice: f64,
    /// Percentage (0-100) of ticks spent on system programs.
    pub system: f64,
    /// Percentage (0-100) of ticks spent idling.
    pub idle: f64,
    /// Percentage (0-100) of ticks spent waiting for IO.
    pub iowait: f64,
    /// Percentage (0-100) of ticks spent on hardware interrupts.
    pub irq: f64,
    /// Percentage (0-100) of ticks spent on software interrupts.
    pub softirq: f64,
    /// Percentage (0-100) of ticks stolen by others.
    pub steal: f64,
    /// Percentage (0-100) of ticks spent by a guest.
    pub guest: f64,
    /// Percentage (0-100) of ticks spent by a `nice`d guest.
    pub guestnice: f64,
    /// Total. Always 100.
    pub total: f64,
}

/// CPU loads of **online** CPUs, keyed by CPU id (e.g. `cpu0`).
///
/// Ported from `src/CpuStats.ts`'s `Loads`.
pub type Loads = HashMap<String, CpuStats>;

use crate::error::Error;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_type() {
        assert_eq!(DeviceType::parse("emulator"), DeviceType::Emulator);
        assert_eq!(DeviceType::parse("device"), DeviceType::Device);
        assert_eq!(DeviceType::parse("offline"), DeviceType::Offline);
        assert_eq!(DeviceType::parse("unauthorized"), DeviceType::Unauthorized);
        assert_eq!(DeviceType::parse("whatever"), DeviceType::Unknown);
        assert_eq!(DeviceType::Device.to_string(), "device");
    }

    #[test]
    fn test_default_client_options() {
        let opts = ClientOptions::default();
        assert_eq!(opts.host, None);
        assert_eq!(opts.port, None);
        assert_eq!(opts.bin, "adb");
        assert_eq!(opts.timeout, 0);
    }
}
