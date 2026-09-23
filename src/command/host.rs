//! Commands that talk to the ADB server itself (`host:*`).
//!
//! Ported from `src/adb/command/host/*.ts`: `version.ts`, `connect.ts`,
//! `disconnect.ts`, `devices.ts`, `deviceswithpaths.ts`, `trackdevices.ts`,
//! `kill.ts` and `transport.ts`.

use crate::connection::{AdbStream, Connection};
use crate::error::{Error, Result};
use crate::parser::{read_reply, Parser};
use crate::protocol::{Protocol, Reply};
use crate::tracker::TrackerHandle;
use crate::types::{Device, DeviceType, DeviceWithPath};

use super::send;

/// `adb host:version`
///
/// Queries the ADB server for its version. Mainly useful for
/// backwards-compatibility purposes.
///
/// Ported from `HostVersionCommand`.
pub async fn version<S: AdbStream>(connection: &mut Connection<S>) -> Result<u32> {
    send(connection, "host:version").await?;
    let reply = connection.parser().read_ascii(4).await?;
    match reply.as_str() {
        Protocol::OKAY => {
            let value = connection.parser().read_value().await?;
            parse_version(&String::from_utf8_lossy(&value))
        }
        Protocol::FAIL => Err(connection.parser().read_error().await),
        // Old-style servers reply with the version directly, without OKAY.
        _ => parse_version(&reply),
    }
}

fn parse_version(version: &str) -> Result<u32> {
    // `parseInt(value, 16)` accepts an optional `0x` prefix, so we do too.
    let version = version.trim();
    let version = version
        .strip_prefix("0x")
        .or_else(|| version.strip_prefix("0X"))
        .unwrap_or(version);
    u32::from_str_radix(version, 16)
        .map_err(|_| Error::unexpected(version.to_string(), "a hexadecimal version number"))
}

/// `adb host:connect:<host>:<port>`
///
/// Ported from `HostConnectCommand`.
pub async fn connect<S: AdbStream>(
    connection: &mut Connection<S>,
    host: &str,
    port: u16,
) -> Result<String> {
    send(connection, &format!("host:connect:{}:{}", host, port)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_value().await?;
            let value = String::from_utf8_lossy(&value).into_owned();
            if value.contains("connected to") || value.contains("already connected") {
                Ok(format!("{}:{}", host, port))
            } else {
                Err(Error::Device(value))
            }
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host:disconnect:<host>:<port>`
///
/// Ported from `HostDisconnectCommand`.
pub async fn disconnect<S: AdbStream>(
    connection: &mut Connection<S>,
    host: &str,
    port: u16,
) -> Result<String> {
    send(connection, &format!("host:disconnect:{}:{}", host, port)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_value().await?;
            let value = String::from_utf8_lossy(&value).into_owned();
            if value.is_empty() || value.starts_with("disconnected") {
                Ok(format!("{}:{}", host, port))
            } else {
                Err(Error::Device(value))
            }
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host:devices`
///
/// Gets the list of currently connected devices and emulators.
///
/// Ported from `HostDevicesCommand`.
pub async fn devices<S: AdbStream>(connection: &mut Connection<S>) -> Result<Vec<Device>> {
    send(connection, "host:devices").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => read_devices(connection.parser()).await,
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// Reads a device list without sending a command.
///
/// Used by the tracker, which keeps reading on the same connection.
///
/// Ported from `HostDevicesCommand._readDevices()`.
pub async fn read_devices<R>(parser: &mut Parser<R>) -> Result<Vec<Device>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let value = parser.read_value().await?;
    Ok(parse_devices(&String::from_utf8_lossy(&value)))
}

fn parse_devices(value: &str) -> Vec<Device> {
    value
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut parts = line.split('\t');
            let id = parts.next().unwrap_or("").to_string();
            let r#type = DeviceType::parse(parts.next().unwrap_or(""));
            Device { id, r#type }
        })
        .collect()
}

/// `adb host:devices-l`
///
/// Like [`devices`], but includes the "path" of every device.
///
/// Ported from `HostDevicesWithPathsCommand`.
pub async fn devices_with_paths<S: AdbStream>(
    connection: &mut Connection<S>,
) -> Result<Vec<DeviceWithPath>> {
    send(connection, "host:devices-l").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => read_devices_with_paths(connection.parser()).await,
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// Reads a device list with paths, without sending a command.
///
/// Ported from `HostDevicesWithPathsCommand._readDevices()`.
pub async fn read_devices_with_paths<R>(parser: &mut Parser<R>) -> Result<Vec<DeviceWithPath>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let value = parser.read_value().await?;
    Ok(parse_devices_with_paths(&String::from_utf8_lossy(&value)))
}

fn parse_devices_with_paths(value: &str) -> Vec<DeviceWithPath> {
    value
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            // For some reason, the columns are separated by spaces instead of
            // tabs.
            let mut columns: Vec<&str> = line.split_whitespace().collect();
            columns.resize(7, "");
            DeviceWithPath {
                id: columns[0].to_string(),
                r#type: DeviceType::parse(columns[1]),
                path: columns[2].to_string(),
                product: columns[3].to_string(),
                model: columns[4].to_string(),
                device: columns[5].to_string(),
                transport_id: columns[6].to_string(),
            }
        })
        .collect()
}

/// `adb host:kill`
///
/// Kills the ADB server.
///
/// Ported from `HostKillCommand`.
pub async fn kill<S: AdbStream>(connection: &mut Connection<S>) -> Result<bool> {
    send(connection, "host:kill").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(true),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host:transport:<serial>`
///
/// Selects the device that subsequent commands talk to. The connection is kept
/// open, so this is called before every device command.
///
/// Ported from `HostTransportCommand`.
pub async fn transport<S: AdbStream>(connection: &mut Connection<S>, serial: &str) -> Result<bool> {
    send(connection, &format!("host:transport:{}", serial)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(true),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host:track-devices`
///
/// Ported from `HostTrackDevicesCommand.execute()`. Returns a tracker that
/// emits `add`, `remove`, `change`, `changeSet` and `end` events.
pub async fn track_devices<S: AdbStream>(mut connection: Connection<S>) -> Result<TrackerHandle> {
    send(&mut connection, "host:track-devices").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(TrackerHandle::spawn(connection)),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_devices() {
        let devices = parse_devices("a\tdevice\nb\toffline\n");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "a");
        assert_eq!(devices[0].r#type, DeviceType::Device);
        assert_eq!(devices[1].id, "b");
        assert_eq!(devices[1].r#type, DeviceType::Offline);
    }

    #[test]
    fn test_parse_devices_with_paths() {
        let devices = parse_devices_with_paths(
            "1234567890 device usb:1-1 product model device transport_id\n",
        );
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "1234567890");
        assert_eq!(devices[0].r#type, DeviceType::Device);
        assert_eq!(devices[0].path, "usb:1-1");
        assert_eq!(devices[0].product, "product");
        assert_eq!(devices[0].model, "model");
        assert_eq!(devices[0].device, "device");
        assert_eq!(devices[0].transport_id, "transport_id");
    }
}
