//! Commands for the ADB server and devices.
//!
//! The TypeScript implementation has an abstract `Command` base class
//! (`src/adb/command.ts`) that every command extends, with the concrete
//! commands living in `src/adb/command/{host,host-serial,host-transport}/`.
//! In Rust those are async functions grouped by module:
//!
//! -   [`host`]: commands that talk to the ADB server itself (`host:*`).
//! -   [`host_serial`]: commands that select a device by serial
//!     (`host-serial:*`).
//! -   [`host_transport`]: commands that talk to a device (`shell:*`, `sync:`,
//!     `tcp:`, …).

pub mod host;
pub mod host_serial;
pub mod host_transport;

pub use self::host::*;
pub use self::host_serial::*;
pub use self::host_transport::*;

use crate::connection::{AdbStream, Connection};
use crate::error::Result;
use crate::protocol::Protocol;

/// Sends the given data to the ADB server, length-prefixed.
///
/// Mirrors `Command._send()`.
pub async fn send<S: AdbStream>(connection: &mut Connection<S>, data: &str) -> Result<()> {
    crate::debug!("Send '{}'", data);
    connection
        .write(&Protocol::encode_data(data.as_bytes()))
        .await
}

/// An argument that can be escaped for a shell command.
///
/// Mirrors the `number | WithToString` union accepted by `Command._escape()`.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// A string argument, which will be quoted.
    Str(String),
    /// A number argument, which will be passed through as-is.
    Num(i64),
}

impl std::fmt::Display for Arg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arg::Str(value) => f.write_str(value),
            Arg::Num(value) => write!(f, "{}", value),
        }
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Arg::Str(value.to_string())
    }
}

impl From<String> for Arg {
    fn from(value: String) -> Self {
        Arg::Str(value)
    }
}

impl From<i64> for Arg {
    fn from(value: i64) -> Self {
        Arg::Num(value)
    }
}

impl From<i32> for Arg {
    fn from(value: i32) -> Self {
        Arg::Num(value as i64)
    }
}

impl From<u32> for Arg {
    fn from(value: u32) -> Self {
        Arg::Num(value as i64)
    }
}

impl From<bool> for Arg {
    fn from(value: bool) -> Self {
        Arg::Str(value.to_string())
    }
}

/// Escapes an argument for use in a shell command.
///
/// Mirrors `Command._escape()`. Numbers are passed through, everything else is
/// wrapped in single quotes (with `'` escaped as `'"'"'`).
pub fn escape(arg: impl Into<Arg>) -> String {
    match arg.into() {
        Arg::Num(value) => value.to_string(),
        Arg::Str(value) => format!("'{}'", value.replace('\'', "'\"'\"'")),
    }
}

/// Escapes an argument in a way that is compatible with older shells.
///
/// Mirrors `Command._escapeCompat()`: numbers are passed through, everything
/// else is wrapped in double quotes with `$`, backtick, `\` and `!"` escaped.
pub fn escape_compat(arg: impl Into<Arg>) -> String {
    let re = regex::Regex::new(r#"([$`\\!"])"#).unwrap();
    match arg.into() {
        Arg::Num(value) => value.to_string(),
        Arg::Str(value) => format!("\"{}\"", re.replace_all(&value, "\\$1")),
    }
}

/// The extra types understood by `am start`/`am startservice`.
///
/// Mirrors `EXTRA_TYPES` in
/// `src/adb/command/host-transport/startactivity.ts`.
pub fn extra_type(name: &str) -> Option<&'static str> {
    Some(match name {
        "string" => "s",
        "null" => "sn",
        "bool" => "z",
        "int" => "i",
        "long" => "l",
        "float" => "f",
        "uri" => "u",
        "component" => "cn",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape() {
        assert_eq!(escape(Arg::Num(5)), "5");
        assert_eq!(escape(Arg::Str("foo".into())), "'foo'");
        assert_eq!(escape(Arg::Str("it's".into())), "'it'\"'\"'s'");
        assert_eq!(escape("bar"), "'bar'");
        assert_eq!(escape(5u32), "5");
    }

    #[test]
    fn test_escape_compat() {
        assert_eq!(escape_compat(Arg::Num(5)), "5");
        assert_eq!(escape_compat(Arg::Str("foo".into())), "\"foo\"");
        assert_eq!(
            escape_compat(Arg::Str("a$b`c\\d!e\"f".into())),
            "\"a\\$b\\`c\\\\d\\!e\\\"f\""
        );
    }

    #[test]
    fn test_extra_type() {
        assert_eq!(extra_type("string"), Some("s"));
        assert_eq!(extra_type("null"), Some("sn"));
        assert_eq!(extra_type("bool"), Some("z"));
        assert_eq!(extra_type("int"), Some("i"));
        assert_eq!(extra_type("component"), Some("cn"));
        assert_eq!(extra_type("nope"), None);
    }

    #[tokio::test]
    async fn test_send_length_prefixes_data() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut connection = Connection::new(client, crate::types::ClientOptions::default());
        send(&mut connection, "host:version").await.unwrap();

        let mut buf = vec![0u8; 32];
        let read = tokio::io::AsyncReadExt::read(&mut server, &mut buf)
            .await
            .unwrap();
        assert_eq!(&buf[..read], b"000Chost:version");
    }
}
