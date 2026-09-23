//! Replies sent by the `monkey` utility.
//!
//! Ported from `lib/monkey/reply.js`.

use crate::error::Error;

/// The type of a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyType {
    /// `OK`
    Ok,
    /// `ERROR`
    Error,
}

impl ReplyType {
    /// The type as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReplyType::Ok => "OK",
            ReplyType::Error => "ERROR",
        }
    }
}

impl std::fmt::Display for ReplyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A reply to a command sent to the `monkey` utility.
///
/// Ported from the `Reply` class of `lib/monkey/reply.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The type of the reply.
    pub r#type: ReplyType,
    /// The value of the reply, if any.
    pub value: Option<String>,
}

impl Reply {
    /// Creates a new reply.
    pub fn new(r#type: ReplyType, value: Option<String>) -> Self {
        Reply { r#type, value }
    }

    /// Whether this reply is an error.
    pub fn is_error(&self) -> bool {
        self.r#type == ReplyType::Error
    }

    /// Turns an error reply into an [`Error`].
    ///
    /// Ported from `Reply.toError()`.
    pub fn to_error(&self) -> Error {
        if !self.is_error() {
            return Error::Device("to_error() cannot be called for non-errors".to_string());
        }
        Error::Device(self.value.clone().unwrap_or_default())
    }
}

impl std::fmt::Display for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.value {
            Some(value) => write!(f, "{} {}", self.r#type, value),
            None => write!(f, "{}", self.r#type),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ok_reply() {
        let reply = Reply::new(ReplyType::Ok, None);
        assert!(!reply.is_error());
        assert_eq!(reply.to_string(), "OK");
    }

    #[test]
    fn test_error_reply() {
        let reply = Reply::new(ReplyType::Error, Some("Invalid command".into()));
        assert!(reply.is_error());
        assert_eq!(reply.to_error().to_string(), "Invalid command");
        assert_eq!(reply.to_string(), "ERROR Invalid command");
    }
}
