//! Error and result types.
//!
//! Ported from the various `Error` subclasses used throughout the TypeScript
//! implementation (`Parser.FailError`, `Parser.PrematureEOFError`,
//! `Parser.UnexpectedDataError`, `Parser.TimeoutError`, `Socket.AuthError`,
//! `Socket.UnauthorizedError`, `PacketReader.ChecksumError`,
//! `PacketReader.MagicError`, `Service.PrematurePacketError` and friends).

/// Every error that can be produced by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O error on the underlying socket.
    #[error("{0}")]
    Io(#[from] std::io::Error),

    /// The ADB server replied with a `FAIL` message.
    ///
    /// Mirrors `Parser.FailError`, whose message was `Failure: '<message>'`.
    #[error("Failure: '{0}'")]
    Failure(String),

    /// Unexpected data was received where something else was expected.
    #[error("Unexpected '{unexpected}', was expecting {expected}")]
    Unexpected {
        /// The data that was actually received.
        unexpected: String,
        /// A description of what was expected instead.
        expected: String,
    },

    /// The stream ended before the requested amount of bytes was read.
    ///
    /// Mirrors `Parser.PrematureEOFError`.
    #[error("Premature end of stream, needed {0} more bytes")]
    PrematureEof(usize),

    /// The socket timeout (see `ClientOptions::timeout`) was reached.
    #[error("Socket timeout reached.")]
    Timeout,

    /// Authentication failed while acting as an ADB server.
    #[error("{0}")]
    Auth(String),

    /// A client tried to use a service before authorizing the connection.
    #[error("Unauthorized access")]
    Unauthorized,

    /// A TCP packet had a bad checksum.
    #[error("Checksum mismatch")]
    Checksum,

    /// A TCP packet had a bad magic value.
    #[error("Magic value mismatch")]
    Magic,

    /// A packet arrived before it was expected.
    #[error("Premature packet")]
    PrematurePacket,

    /// The transport connection arrived too late (the service already ended).
    #[error("Late transport")]
    LateTransport,

    /// An APK could not be installed.
    #[error("{message}")]
    Install {
        /// Human readable message.
        message: String,
        /// The failure code reported by the device.
        code: String,
    },

    /// A package could not be uninstalled.
    #[error("{0}")]
    Uninstall(String),

    /// Any other error, with a device/adb provided message.
    #[error("{0}")]
    Device(String),

    /// A key could not be parsed.
    #[error("{0}")]
    Key(String),
}

impl Error {
    /// Creates an [`Error::Unexpected`] from anything stringifiable.
    pub fn unexpected<U: Into<String>, E: Into<String>>(unexpected: U, expected: E) -> Self {
        Error::Unexpected {
            unexpected: unexpected.into(),
            expected: expected.into(),
        }
    }

    /// Whether this error means that the `--user` option is unsupported.
    ///
    /// `startActivity` and `startService` retry without the user option when
    /// the device complains about it, mirroring the `NoUserOptionError` helper
    /// in `src/adb/DeviceClient.ts`.
    pub fn is_no_user_option(&self) -> bool {
        match self {
            Error::Device(message) | Error::Uninstall(message) | Error::Failure(message) => {
                message.contains("--user")
            }
            _ => false,
        }
    }

    /// The kind of I/O error, if this is an [`Error::Io`].
    pub fn io_kind(&self) -> Option<std::io::ErrorKind> {
        match self {
            Error::Io(err) => Some(err.kind()),
            _ => None,
        }
    }

    /// Whether this error is an I/O error of the given kind.
    pub fn is_io_kind(&self, kind: std::io::ErrorKind) -> bool {
        matches!(self, Error::Io(err) if err.kind() == kind)
    }

    /// Whether this error means the ADB server refused the connection.
    ///
    /// Mirrors the `ECONNREFUSED` check in `Connection.connect()`.
    pub fn is_connection_refused(&self) -> bool {
        self.is_io_kind(std::io::ErrorKind::ConnectionRefused)
    }
}

/// A specialized [`Result`] type for this crate.
pub type Result<T> = std::result::Result<T, Error>;

impl From<std::num::ParseIntError> for Error {
    fn from(err: std::num::ParseIntError) -> Self {
        Error::Device(err.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(err: std::str::Utf8Error) -> Self {
        Error::Device(err.to_string())
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(err: std::string::FromUtf8Error) -> Self {
        Error::Device(err.to_string())
    }
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Error::Io(a), Error::Io(b)) => a.kind() == b.kind(),
            (Error::Failure(a), Error::Failure(b)) => a == b,
            (
                Error::Unexpected {
                    unexpected: u1,
                    expected: e1,
                },
                Error::Unexpected {
                    unexpected: u2,
                    expected: e2,
                },
            ) => u1 == u2 && e1 == e2,
            (Error::PrematureEof(a), Error::PrematureEof(b)) => a == b,
            (Error::Timeout, Error::Timeout) => true,
            (Error::Auth(a), Error::Auth(b)) => a == b,
            (Error::Unauthorized, Error::Unauthorized) => true,
            (Error::Checksum, Error::Checksum) => true,
            (Error::Magic, Error::Magic) => true,
            (Error::PrematurePacket, Error::PrematurePacket) => true,
            (Error::LateTransport, Error::LateTransport) => true,
            (
                Error::Install {
                    message: m1,
                    code: c1,
                },
                Error::Install {
                    message: m2,
                    code: c2,
                },
            ) => m1 == m2 && c1 == c2,
            (Error::Uninstall(a), Error::Uninstall(b)) => a == b,
            (Error::Device(a), Error::Device(b)) => a == b,
            (Error::Key(a), Error::Key(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_message() {
        assert_eq!(
            Error::Failure("nope".to_string()).to_string(),
            "Failure: 'nope'"
        );
    }

    #[test]
    fn test_unexpected_message() {
        assert_eq!(
            Error::unexpected("BABA", "OKAY or FAIL").to_string(),
            "Unexpected 'BABA', was expecting OKAY or FAIL"
        );
    }

    #[test]
    fn test_premature_eof_message() {
        assert_eq!(
            Error::PrematureEof(12).to_string(),
            "Premature end of stream, needed 12 more bytes"
        );
    }

    #[test]
    fn test_no_user_option() {
        assert!(Error::Device("Error: --user is not supported".to_string()).is_no_user_option());
        assert!(!Error::Device("some other problem".to_string()).is_no_user_option());
    }

    #[test]
    fn test_connection_refused() {
        let err = Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
        assert!(err.is_connection_refused());
        assert!(!Error::Timeout.is_connection_refused());
    }
}
