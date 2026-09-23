//! Commands that talk to a device (`transport:*`, `shell:*`, `sync:` …).
//!
//! Ported from `src/adb/command/host-transport/*.ts`: `clear.ts`,
//! `framebuffer.ts`, `getfeatures.ts`, `getpackages.ts`, `getproperties.ts`,
//! `install.ts`, `isinstalled.ts`, `listreverses.ts`, `local.ts`, `log.ts`,
//! `logcat.ts`, `monkey.ts`, `reboot.ts`, `remount.ts`, `reverse.ts`,
//! `root.ts`, `screencap.ts`, `shell.ts`, `startactivity.ts`,
//! `startservice.ts`, `sync.ts`, `tcp.ts`, `tcpip.ts`, `trackjdwp.ts`,
//! `uninstall.ts`, `usb.ts` and `waitbootcomplete.ts`.

use std::time::Duration;

use regex::Regex;

use crate::connection::{AdbStream, Connection, RawStream};
use crate::error::{Error, Result};
use crate::framebuffer::{self, FramebufferStream};
use crate::jdwptracker::JdwpTracker;
use crate::linetransform::LineTransform;
use crate::parser::read_reply;
use crate::protocol::Reply;
use crate::sync::Sync;
use crate::types::{
    ExtraValue, Extras, Features, Properties, Reverse, StartActivityOptions, StartServiceOptions,
};

use super::{escape, send};

/// The timeout used when waiting for the `monkey` banner.
const MONKEY_TIMEOUT: Duration = Duration::from_millis(1000);

/// `shell:pm clear <pkg>`
///
/// Deletes all data associated with a package from the device.
pub async fn clear<S: AdbStream>(connection: &mut Connection<S>, pkg: &str) -> Result<bool> {
    send(connection, &format!("shell:pm clear {}", pkg)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let re = Regex::new(r"^(Success|Failed)$").unwrap();
            let result = match connection.parser().search_line(&re).await {
                Ok(groups) => match groups.get(1).and_then(|g| g.clone()) {
                    Some(line) if line == "Success" => Ok(true),
                    Some(_) => Err(Error::Device(format!(
                        "Package '{}' could not be cleared",
                        pkg
                    ))),
                    None => Ok(false),
                },
                Err(err) => Err(err),
            };
            let _ = connection.parser().end().await;
            result
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:pm list features 2>/dev/null`
///
/// Retrieves the features of the device.
pub async fn get_features<S: AdbStream>(connection: &mut Connection<S>) -> Result<Features> {
    send(connection, "shell:pm list features 2>/dev/null").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let data = connection.parser().read_all().await?;
            Ok(parse_features(&String::from_utf8_lossy(&data)))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

fn parse_features(value: &str) -> Features {
    let re = Regex::new(r"(?m)^feature:(.*?)(?:=(.*?))?\r?$").unwrap();
    let mut features = Features::new();
    for captures in re.captures_iter(value) {
        let key = captures
            .get(1)
            .map(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let value = match captures.get(2) {
            Some(value) => !value.as_str().is_empty(),
            None => true,
        };
        features.insert(key, value);
    }
    features
}

/// `shell:pm list packages 2>/dev/null`
///
/// Retrieves the list of packages present on the device.
pub async fn get_packages<S: AdbStream>(
    connection: &mut Connection<S>,
    flags: Option<&str>,
) -> Result<Vec<String>> {
    match flags {
        Some(flags) => {
            send(
                connection,
                &format!("shell:pm list packages {} 2>/dev/null", flags),
            )
            .await?
        }
        None => send(connection, "shell:pm list packages 2>/dev/null").await?,
    }
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let data = connection.parser().read_all().await?;
            Ok(parse_packages(&String::from_utf8_lossy(&data)))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

fn parse_packages(value: &str) -> Vec<String> {
    let re = Regex::new(r"(?m)^package:(.*?)\r?$").unwrap();
    re.captures_iter(value)
        .map(|captures| {
            captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

/// `shell:getprop`
///
/// Retrieves the properties of the device.
pub async fn get_properties<S: AdbStream>(connection: &mut Connection<S>) -> Result<Properties> {
    send(connection, "shell:getprop").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let data = connection.parser().read_all().await?;
            Ok(parse_properties(&String::from_utf8_lossy(&data)))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

fn parse_properties(value: &str) -> Properties {
    let re = Regex::new(r"(?m)^\[([\s\S]*?)\]: \[([\s\S]*?)\]\r?$").unwrap();
    let mut properties = Properties::new();
    for captures in re.captures_iter(value) {
        let key = captures
            .get(1)
            .map(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let value = captures
            .get(2)
            .map(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        properties.insert(key, value);
    }
    properties
}

/// `shell:pm install -r <apk>`
///
/// Installs an APK file which must already be located on the device file
/// system. The file is removed when the command completes.
pub async fn install<S: AdbStream>(connection: &mut Connection<S>, apk: &str) -> Result<bool> {
    send(
        connection,
        &format!("shell:pm install -r {}", super::escape_compat(apk)),
    )
    .await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let re = Regex::new(r"^(Success|Failure \[(.*?)\]|Exception)(.*)$").unwrap();
            let groups = connection.parser().search_line(&re).await?;
            let result = match groups.get(1).and_then(|g| g.clone()) {
                Some(line) if line == "Success" => Ok(true),
                Some(line) if line == "Exception" => {
                    let buffer = connection.parser().read_line().await?;
                    Err(Error::Install {
                        message: String::from_utf8_lossy(&buffer).into_owned(),
                        code: "INSTALL_EXCEPTION".to_string(),
                    })
                }
                Some(_) => {
                    let code = groups.get(2).and_then(|g| g.clone()).unwrap_or_default();
                    Err(Error::Install {
                        message: format!("{} could not be installed [{}]", apk, code),
                        code,
                    })
                }
                None => Ok(false),
            };
            let _ = connection.parser().read_all().await;
            result
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:pm path <pkg> 2>/dev/null`
///
/// Tells you if the specific package is installed or not.
pub async fn is_installed<S: AdbStream>(connection: &mut Connection<S>, pkg: &str) -> Result<bool> {
    send(connection, &format!("shell:pm path {} 2>/dev/null", pkg)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let reply = connection.parser().read_ascii(8).await;
            match reply {
                Ok(reply) if reply == "package:" => Ok(true),
                Ok(reply) => connection.parser().unexpected(reply, "'package:'"),
                Err(Error::PrematureEof(_)) => Ok(false),
                Err(err) => Err(err),
            }
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `reverse:list-forward`
///
/// Lists reversed connections on the device.
pub async fn list_reverses<S: AdbStream>(connection: &mut Connection<S>) -> Result<Vec<Reverse>> {
    send(connection, "reverse:list-forward").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_value().await?;
            Ok(parse_reverses(&String::from_utf8_lossy(&value)))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

fn parse_reverses(value: &str) -> Vec<Reverse> {
    let mut reverses = Vec::new();
    for line in value.split('\n') {
        if line.is_empty() {
            continue;
        }
        let mut columns: Vec<&str> = line.split_whitespace().collect();
        columns.resize(3, "");
        reverses.push(Reverse {
            remote: columns[1].to_string(),
            local: columns[2].to_string(),
        });
    }
    reverses
}

/// `localfilesystem:<path>` (or the path itself when it already has a scheme)
///
/// Opens a direct connection to a unix domain socket in the given path.
pub async fn open_local<S: AdbStream>(
    mut connection: Connection<S>,
    path: &str,
) -> Result<RawStream<S>> {
    let service = if path.contains(':') {
        path.to_string()
    } else {
        format!("localfilesystem:{}", path)
    };
    send(&mut connection, &service).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => connection.into_raw_stream(),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `log:<name>`
///
/// Opens a direct connection to a binary log file.
pub async fn open_log<S: AdbStream>(
    mut connection: Connection<S>,
    name: &str,
) -> Result<RawStream<S>> {
    send(&mut connection, &format!("log:{}", name)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => connection.into_raw_stream(),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:echo && logcat -B *:I 2>/dev/null`
///
/// Calls the `logcat` utility on the device. The returned stream has been
/// filtered through a [`LineTransform`], ready to be handed to
/// [`crate::logcat::Reader`].
pub async fn logcat<S: AdbStream>(
    mut connection: Connection<S>,
    clear: bool,
) -> Result<LineTransform<RawStream<S>>> {
    let mut cmd = String::from("logcat -B *:I 2>/dev/null");
    if clear {
        cmd = format!("logcat -c 2>/dev/null && {}", cmd);
    }
    send(&mut connection, &format!("shell:echo && {}", cmd)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let stream = connection.into_raw_stream()?;
            Ok(LineTransform::with_options(stream, true))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:EXTERNAL_STORAGE=/data/local/tmp monkey --port <port> -v`
///
/// Starts the built-in `monkey` utility on the device.
pub async fn monkey<S: AdbStream>(
    mut connection: Connection<S>,
    port: u16,
) -> Result<RawStream<S>> {
    send(
        &mut connection,
        &format!(
            "shell:EXTERNAL_STORAGE=/data/local/tmp monkey --port {} -v",
            port
        ),
    )
    .await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            // The monkey command is a bit weird in that it doesn't look like it
            // starts in daemon mode, but it actually does. On some devices the
            // command gives no output no matter how many verbose flags you
            // give it, so we use a fallback timeout.
            let re = Regex::new(r"^:Monkey:").unwrap();
            match connection
                .parser()
                .search_line_with_timeout(&re, Some(MONKEY_TIMEOUT))
                .await
            {
                Ok(_) => connection.into_raw_stream(),
                Err(Error::Timeout) => connection.into_raw_stream(),
                Err(err) => Err(err),
            }
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `reboot:`
///
/// Reboots the device.
pub async fn reboot<S: AdbStream>(connection: &mut Connection<S>) -> Result<bool> {
    send(connection, "reboot:").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let _ = connection.parser().read_all().await?;
            Ok(true)
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `remount:`
///
/// Attempts to remount the `/system` partition in read-write mode.
pub async fn remount<S: AdbStream>(connection: &mut Connection<S>) -> Result<bool> {
    send(connection, "remount:").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(true),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `reverse:forward:<remote>;<local>`
///
/// Reverses socket connections from the device to the ADB server host.
pub async fn reverse<S: AdbStream>(
    connection: &mut Connection<S>,
    remote: &str,
    local: &str,
) -> Result<bool> {
    send(connection, &format!("reverse:forward:{};{}", remote, local)).await?;
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

/// `root:`
///
/// Puts the device into root mode.
pub async fn root<S: AdbStream>(connection: &mut Connection<S>) -> Result<bool> {
    send(connection, "root:").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_all().await?;
            check_restarting(&String::from_utf8_lossy(&value))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:echo && screencap -p 2>/dev/null`
///
/// Takes a screenshot in PNG format.
pub async fn screencap<S: AdbStream>(
    mut connection: Connection<S>,
) -> Result<LineTransform<RawStream<S>>> {
    send(&mut connection, "shell:echo && screencap -p 2>/dev/null").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let chunk = match connection.parser().read_bytes(1).await {
                Ok(chunk) => chunk,
                Err(Error::PrematureEof(_)) => {
                    return Err(Error::Device(
                        "No support for the screencap command".to_string(),
                    ))
                }
                Err(err) => return Err(err),
            };
            let stream = connection.into_raw_stream()?;
            let mut transform = LineTransform::with_options(stream, true);
            transform.feed(&chunk);
            Ok(transform)
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:<command>`
///
/// Runs a shell command on the device. The command is limited to the
/// permissions of the `shell` user, which ADB uses.
pub async fn shell<S: AdbStream>(
    mut connection: Connection<S>,
    command: &str,
) -> Result<RawStream<S>> {
    send(&mut connection, &format!("shell:{}", command)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => connection.into_raw_stream(),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// Escapes and joins the given arguments into a shell command.
///
/// Ported from the `Array` branch of `ShellCommand.execute()`.
pub fn shell_command(args: &[super::Arg]) -> String {
    args.iter()
        .map(|arg| escape(arg.clone()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `sync:`
///
/// Establishes a new sync connection that can be used to push and pull files.
pub async fn sync_service<S: AdbStream>(mut connection: Connection<S>) -> Result<Sync<S>> {
    send(&mut connection, "sync:").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(Sync::new(connection)),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `tcp:<port>`
///
/// Opens a direct TCP connection to a port on the device, without any port
/// forwarding required.
pub async fn open_tcp<S: AdbStream>(
    mut connection: Connection<S>,
    port: u16,
    host: Option<&str>,
) -> Result<RawStream<S>> {
    let service = match host {
        Some(host) => format!("tcp:{}:{}", port, host),
        None => format!("tcp:{}", port),
    };
    send(&mut connection, &service).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => connection.into_raw_stream(),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `tcpip:<port>`
///
/// Puts the device's ADB daemon into tcp mode.
pub async fn tcpip<S: AdbStream>(connection: &mut Connection<S>, port: u16) -> Result<u16> {
    send(connection, &format!("tcpip:{}", port)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_all().await?;
            check_restarting_port(&String::from_utf8_lossy(&value), port)
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

fn check_restarting(value: &str) -> Result<bool> {
    if value.contains("restarting in") || value.contains("restarting adbd as root") {
        Ok(true)
    } else {
        Err(Error::Device(value.trim().to_string()))
    }
}

fn check_restarting_port(value: &str, port: u16) -> Result<u16> {
    if value.contains("restarting in") || value.contains("restarting adbd as root") {
        Ok(port)
    } else {
        Err(Error::Device(value.trim().to_string()))
    }
}

/// `track-jdwp`
///
/// Starts a JDWP tracker for the device.
pub async fn track_jdwp<S: AdbStream>(mut connection: Connection<S>) -> Result<JdwpTracker> {
    send(&mut connection, "track-jdwp").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => Ok(JdwpTracker::from_connection(connection)),
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:pm uninstall <pkg>`
///
/// Uninstalls the package from the device.
pub async fn uninstall<S: AdbStream>(connection: &mut Connection<S>, pkg: &str) -> Result<bool> {
    send(connection, &format!("shell:pm uninstall {}", pkg)).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let re = Regex::new(r"^(Success|Failure.*|.*Unknown package:.*)$").unwrap();
            let groups = match connection.parser().search_line(&re).await {
                Ok(groups) => groups,
                Err(err) => {
                    let _ = connection.parser().read_all().await;
                    return Err(err);
                }
            };
            let result = match groups.get(1).and_then(|g| g.clone()) {
                Some(line) if line == "Success" => Ok(true),
                Some(line) if line.contains("DELETE_FAILED_DEVICE_POLICY_MANAGER") => Err(
                    Error::Uninstall(format!("{} could not be uninstalled [{}]", pkg, line)),
                ),
                // Either way, the package was uninstalled or doesn't exist,
                // which is good enough for us.
                _ => Ok(true),
            };
            let _ = connection.parser().read_all().await;
            result
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `usb:`
///
/// Puts the device's ADB daemon back into USB mode.
pub async fn usb<S: AdbStream>(connection: &mut Connection<S>) -> Result<bool> {
    send(connection, "usb:").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let value = connection.parser().read_all().await?;
            check_restarting(&String::from_utf8_lossy(&value))
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:while getprop sys.boot_completed 2>/dev/null; do sleep 1; done`
///
/// Waits until the device has finished booting.
pub async fn wait_boot_complete<S: AdbStream>(connection: &mut Connection<S>) -> Result<bool> {
    send(
        connection,
        "shell:while getprop sys.boot_completed 2>/dev/null; do sleep 1; done",
    )
    .await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let re = Regex::new(r"^1$").unwrap();
            let _ = connection.parser().search_line(&re).await;
            let _ = connection.parser().end().await;
            Ok(true)
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `framebuffer:`
///
/// Fetches the current **raw** framebuffer from the device, optionally
/// converting it into something more usable using GraphicsMagick's `gm`
/// command.
pub async fn framebuffer<S: AdbStream>(
    mut connection: Connection<S>,
    format: &str,
) -> Result<FramebufferStream<S>> {
    send(&mut connection, "framebuffer:").await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let header = connection
                .parser()
                .read_bytes(framebuffer::HEADER_SIZE)
                .await?;
            let meta = framebuffer::parse_header(&header)?;
            if format == "raw" {
                Ok(FramebufferStream::raw(connection.into_raw_stream()?, meta))
            } else {
                FramebufferStream::convert(meta, format, connection.into_raw_stream()?).await
            }
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

/// `shell:am start <options>`
///
/// Starts the configured activity on the device.
pub async fn start_activity<S: AdbStream>(
    connection: &mut Connection<S>,
    options: &StartActivityOptions,
) -> Result<bool> {
    let mut args = intent_args(&options.service);
    if options.debug {
        args.push(super::Arg::Str("-D".to_string()));
    }
    if options.wait {
        args.push(super::Arg::Str("-W".to_string()));
    }
    run_am(connection, "start", &args).await
}

/// `shell:am startservice <options>`
///
/// Starts the configured service on the device.
pub async fn start_service<S: AdbStream>(
    connection: &mut Connection<S>,
    options: &StartServiceOptions,
) -> Result<bool> {
    let args = intent_args(options);
    run_am(connection, "startservice", &args).await
}

async fn run_am<S: AdbStream>(
    connection: &mut Connection<S>,
    command: &str,
    args: &[super::Arg],
) -> Result<bool> {
    let joined = args
        .iter()
        .map(|arg| arg.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let service = format!("shell:am {} {}", command, joined);
    send(connection, &service).await?;
    match read_reply(connection.parser()).await? {
        Reply::Okay => {
            let re = Regex::new(r"^Error: (.*)$").unwrap();
            let result = match connection.parser().search_line(&re).await {
                Ok(groups) => {
                    let message = groups.get(1).and_then(|g| g.clone()).unwrap_or_default();
                    Err(Error::Device(message))
                }
                // A premature EOF means the command completed without an error.
                Err(Error::PrematureEof(_)) => Ok(true),
                Err(err) => Err(err),
            };
            let _ = connection.parser().end().await;
            result
        }
        reply => connection
            .parser()
            .unexpected(reply.to_string(), "OKAY or FAIL"),
    }
}

fn intent_args(options: &StartServiceOptions) -> Vec<super::Arg> {
    let mut args: Vec<super::Arg> = Vec::new();
    if let Some(extras) = &options.extras {
        args.extend(format_extras(extras));
    }
    if let Some(action) = &options.action {
        args.push(super::Arg::Str("-a".to_string()));
        args.push(super::Arg::Str(escape(action.as_str())));
    }
    if let Some(data) = &options.data {
        args.push(super::Arg::Str("-d".to_string()));
        args.push(super::Arg::Str(escape(data.as_str())));
    }
    if let Some(mime_type) = &options.mime_type {
        args.push(super::Arg::Str("-t".to_string()));
        args.push(super::Arg::Str(escape(mime_type.as_str())));
    }
    if let Some(categories) = &options.category {
        for category in categories {
            args.push(super::Arg::Str("-c".to_string()));
            args.push(super::Arg::Str(escape(category.as_str())));
        }
    }
    if let Some(component) = &options.component {
        args.push(super::Arg::Str("-n".to_string()));
        args.push(super::Arg::Str(escape(component.as_str())));
    }
    if let Some(flags) = &options.flags {
        args.push(super::Arg::Str("-f".to_string()));
        args.push(super::Arg::Str(escape(
            flags
                .iter()
                .map(|flag| flag.to_string())
                .collect::<Vec<_>>()
                .join(","),
        )));
    }
    if let Some(user) = options.user {
        args.push(super::Arg::Str("--user".to_string()));
        args.push(super::Arg::Str(escape(user.to_string())));
    }
    args
}

fn format_extras(extras: &Extras) -> Vec<super::Arg> {
    match extras {
        Extras::List(list) => list.iter().flat_map(format_long_extra).collect(),
        Extras::Map(map) => map
            .iter()
            .flat_map(|(key, value)| format_long_extra(&short_to_extra(key, value)))
            .collect(),
    }
}

fn short_to_extra(key: &str, value: &ExtraValue) -> crate::types::Extra {
    use crate::types::Extra;

    match value {
        ExtraValue::Null => Extra::new(key, ExtraValue::Null),
        other => Extra::new(key, other.clone()),
    }
}

fn format_long_extra(extra: &crate::types::Extra) -> Vec<super::Arg> {
    use crate::types::ExtraValue;

    let mut args = Vec::new();
    let type_name = match &extra.value {
        ExtraValue::String(_) => "string",
        ExtraValue::Bool(_) => "bool",
        ExtraValue::Int(_) => "int",
        ExtraValue::Long(_) => "long",
        ExtraValue::Float(_) => "float",
        ExtraValue::Null => "null",
        ExtraValue::Uri(_) => "uri",
        ExtraValue::Component(_) => "component",
    };
    let type_code = match super::extra_type(type_name) {
        Some(type_code) => type_code,
        None => {
            return vec![];
        }
    };
    args.push(super::Arg::Str(format!("--e{}", type_code)));
    args.push(super::Arg::Str(escape(extra.key.as_str())));
    match &extra.value {
        ExtraValue::Null => {}
        ExtraValue::String(value) => {
            args.push(super::Arg::Str(escape(value.as_str())));
        }
        ExtraValue::Bool(value) => {
            args.push(super::Arg::Str(escape(value.to_string())));
        }
        ExtraValue::Int(value) | ExtraValue::Long(value) => {
            args.push(super::Arg::Str(escape(value.to_string())));
        }
        ExtraValue::Float(value) => {
            args.push(super::Arg::Str(escape(value.to_string())));
        }
        ExtraValue::Uri(value) => {
            args.push(super::Arg::Str(escape(value.as_str())));
        }
        ExtraValue::Component(value) => {
            args.push(super::Arg::Str(escape(value.as_str())));
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Extra;

    #[test]
    fn test_parse_features() {
        let features = parse_features(
            "feature:android.hardware.nfc\r\nfeature:reqGlEsVersion=0x20000\r\nrandom noise",
        );
        assert_eq!(features.get("android.hardware.nfc"), Some(&true));
        assert_eq!(features.get("reqGlEsVersion"), Some(&true));
        assert_eq!(features.len(), 2);
    }

    #[test]
    fn test_parse_packages() {
        let packages = parse_packages("package:com.android.settings\r\npackage:com.foo\r\n");
        assert_eq!(packages, vec!["com.android.settings", "com.foo"]);
    }

    #[test]
    fn test_parse_properties() {
        let properties =
            parse_properties("[ro.product.model]: [Pixel 7]\r\n[ro.build.id]: [TQ3A]\r\n");
        assert_eq!(properties.get("ro.product.model").unwrap(), "Pixel 7");
        assert_eq!(properties.get("ro.build.id").unwrap(), "TQ3A");
    }

    #[test]
    fn test_parse_reverses() {
        let reverses = parse_reverses("serial tcp:1234 localabstract:foo\n");
        assert_eq!(reverses.len(), 1);
        assert_eq!(reverses[0].remote, "tcp:1234");
        assert_eq!(reverses[0].local, "localabstract:foo");
    }

    #[test]
    fn test_shell_command() {
        assert_eq!(shell_command(&[]), "");
        assert_eq!(
            shell_command(&[
                crate::command::Arg::from("rm"),
                crate::command::Arg::from("-f"),
                crate::command::Arg::from("x y")
            ]),
            "'rm' '-f' 'x y'"
        );
    }

    #[test]
    fn test_intent_args() {
        let options = StartServiceOptions {
            user: Some(10),
            action: Some("android.intent.action.VIEW".to_string()),
            data: Some("content://foo".to_string()),
            mime_type: Some("text/plain".to_string()),
            category: Some(vec!["android.intent.category.DEFAULT".to_string()]),
            component: Some("com.foo/.Bar".to_string()),
            flags: Some(vec![0x10000000]),
            extras: Some(Extras::Map(vec![
                ("key".to_string(), ExtraValue::String("value".to_string())),
                ("flag".to_string(), ExtraValue::Bool(true)),
                ("count".to_string(), ExtraValue::Int(42)),
            ])),
        };
        let args = intent_args(&options);
        let command = args
            .iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(command.contains("--es 'key' 'value'"), "{}", command);
        assert!(command.contains("--ez 'flag' 'true'"), "{}", command);
        assert!(command.contains("--ei 'count' '42'"), "{}", command);
        assert!(
            command.contains("-a 'android.intent.action.VIEW'"),
            "{}",
            command
        );
        assert!(command.contains("-d 'content://foo'"), "{}", command);
        assert!(command.contains("-t 'text/plain'"), "{}", command);
        assert!(
            command.contains("-c 'android.intent.category.DEFAULT'"),
            "{}",
            command
        );
        assert!(command.contains("-n 'com.foo/.Bar'"), "{}", command);
        assert!(command.contains("-f '268435456'"), "{}", command);
        assert!(command.contains("--user '10'"), "{}", command);
    }

    #[test]
    fn test_format_long_extra() {
        let extra = Extra::new("key", ExtraValue::Null);
        let args = format_long_extra(&extra);
        let joined = args
            .iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(joined, "--esn 'key'");
    }
}
