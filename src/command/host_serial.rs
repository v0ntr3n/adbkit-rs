//! Commands that select a device by serial (`host-serial:*`).
//!
//! Ported from `src/adb/command/host-serial/*.ts`: `attach.ts`, `detach.ts`,
//! `forward.ts`, `getdevicepath.ts`, `getserialno.ts`, `getstate.ts`,
//! `listforwards.ts` and `waitfordevice.ts`.

use crate::connection::{AdbStream, Connection};
use crate::error::Result;
use crate::parser::read_reply;
use crate::protocol::Reply;
use crate::types::Forward;

use super::send;

/// `adb host-serial:<serial>:attach`
///
/// Reattaches ADB to the device's ADB USB interface.
pub async fn attach<S: AdbStream>(connection: &mut Connection<S>, serial: &str) -> Result<bool> {
    send(connection, &format!("host-serial:{}:attach", serial)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(true),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host-serial:<serial>:detach`
///
/// Detaches ADB's USB interface from ADB.
pub async fn detach<S: AdbStream>(connection: &mut Connection<S>, serial: &str) -> Result<bool> {
    send(connection, &format!("host-serial:{}:detach", serial)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(true),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host-serial:<serial>:forward:<local>;<remote>`
///
/// Forwards socket connections from the ADB server host (local) to the device
/// (remote).
pub async fn forward<S: AdbStream>(
    connection: &mut Connection<S>,
    serial: &str,
    local: &str,
    remote: &str,
) -> Result<bool> {
    send(
        connection,
        &format!("host-serial:{}:forward:{};{}", serial, local, remote),
    )
    .await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => match read_reply(connection.parser()).await? {
            Reply::Okay => Ok(true),
            reply => connection
                .parser()
                .unexpected(reply.to_string(), "OKAY or FAIL"),
        },
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host-serial:<serial>:get-devpath`
///
/// Gets the device path of the device identified by the given serial number.
pub async fn get_device_path<S: AdbStream>(
    connection: &mut Connection<S>,
    serial: &str,
) -> Result<String> {
    send(connection, &format!("host-serial:{}:get-devpath", serial)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_value().await?;
            Ok(String::from_utf8_lossy(&value).into_owned())
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host-serial:<serial>:get-serialno`
///
/// Gets the serial number of the device. With our API this doesn't really make
/// much sense, but it has been implemented for completeness.
pub async fn get_serial_no<S: AdbStream>(
    connection: &mut Connection<S>,
    serial: &str,
) -> Result<String> {
    send(connection, &format!("host-serial:{}:get-serialno", serial)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_value().await?;
            Ok(String::from_utf8_lossy(&value).into_owned())
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host-serial:<serial>:get-state`
///
/// Gets the state of the device, e.g. `device` or `offline`.
pub async fn get_state<S: AdbStream>(
    connection: &mut Connection<S>,
    serial: &str,
) -> Result<String> {
    send(connection, &format!("host-serial:{}:get-state", serial)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_value().await?;
            Ok(String::from_utf8_lossy(&value).into_owned())
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `adb host-serial:<serial>:list-forward`
///
/// Lists forwarded connections on the device.
pub async fn list_forwards<S: AdbStream>(
    connection: &mut Connection<S>,
    serial: &str,
) -> Result<Vec<Forward>> {
    send(connection, &format!("host-serial:{}:list-forward", serial)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_value().await?;
            Ok(parse_forwards(&String::from_utf8_lossy(&value)))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

fn parse_forwards(value: &str) -> Vec<Forward> {
    value
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut columns: Vec<&str> = line.split_whitespace().collect();
            columns.resize(3, "");
            Forward {
                serial: columns[0].to_string(),
                local: columns[1].to_string(),
                remote: columns[2].to_string(),
            }
        })
        .collect()
}

/// `adb host-serial:<serial>:wait-for-any-device`
///
/// Waits until ADB can see the device.
pub async fn wait_for_device<S: AdbStream>(
    connection: &mut Connection<S>,
    serial: &str,
) -> Result<String> {
    send(
        connection,
        &format!("host-serial:{}:wait-for-any-device", serial),
    )
    .await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => match read_reply(connection.parser()).await? {
            Reply::Okay => Ok(serial.to_string()),
            reply => connection
                .parser()
                .unexpected(reply.to_string(), "OKAY or FAIL"),
        },
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_forwards() {
        let forwards = parse_forwards("serial tcp:1234 localabstract:foo\n");
        assert_eq!(forwards.len(), 1);
        assert_eq!(forwards[0].serial, "serial");
        assert_eq!(forwards[0].local, "tcp:1234");
        assert_eq!(forwards[0].remote, "localabstract:foo");
    }

    #[test]
    fn test_parse_forwards_empty() {
        assert!(parse_forwards("").is_empty());
    }
}
