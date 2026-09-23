//! Protocol helpers for the ADB host and sync services.
//!
//! Ported from `src/adb/protocol.ts`.

/// Reply/status codes used by the ADB host protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Okay,
    Fail,
    Stat,
    Sta2,
    List,
    Dent,
    Recv,
    Data,
    Done,
    Send,
    Quit,
}

impl Reply {
    /// The four character code as it appears on the wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            Reply::Okay => "OKAY",
            Reply::Fail => "FAIL",
            Reply::Stat => "STAT",
            Reply::Sta2 => "STA2",
            Reply::List => "LIST",
            Reply::Dent => "DENT",
            Reply::Recv => "RECV",
            Reply::Data => "DATA",
            Reply::Done => "DONE",
            Reply::Send => "SEND",
            Reply::Quit => "QUIT",
        }
    }
}

impl std::fmt::Display for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The ADB protocol helpers.
///
/// In the TypeScript original this is a class with static members; in Rust the
/// associated functions are grouped here instead.
pub struct Protocol;

impl Protocol {
    /// `OKAY`
    pub const OKAY: &'static str = "OKAY";
    /// `FAIL`
    pub const FAIL: &'static str = "FAIL";
    /// `STAT`
    pub const STAT: &'static str = "STAT";
    /// `STA2`
    pub const STA2: &'static str = "STA2";
    /// `LIST`
    pub const LIST: &'static str = "LIST";
    /// `DENT`
    pub const DENT: &'static str = "DENT";
    /// `RECV`
    pub const RECV: &'static str = "RECV";
    /// `DATA`
    pub const DATA: &'static str = "DATA";
    /// `DONE`
    pub const DONE: &'static str = "DONE";
    /// `SEND`
    pub const SEND: &'static str = "SEND";
    /// `QUIT`
    pub const QUIT: &'static str = "QUIT";

    /// Decodes a four character, uppercase hexadecimal length.
    pub fn decode_length(length: &str) -> Result<u32, Error> {
        u32::from_str_radix(length, 16).map_err(|_| Error::Unexpected {
            unexpected: length.to_string(),
            expected: "a hexadecimal length".to_string(),
        })
    }

    /// Encodes a length as a four character, uppercase hexadecimal string.
    ///
    /// Mirrors `length.toString(16).padStart(4, '0').toUpperCase()`. Note that
    /// the TypeScript version happily produces more than four characters for
    /// lengths above `0xffff`; we keep that behaviour for compatibility.
    pub fn encode_length(length: u32) -> String {
        format!("{:04X}", length)
    }

    /// Prefixes the given data with its (hex encoded) length.
    pub fn encode_data<T: AsRef<[u8]>>(data: T) -> Vec<u8> {
        let data = data.as_ref();
        let mut out = Vec::with_capacity(4 + data.len());
        out.extend_from_slice(Self::encode_length(data.len() as u32).as_bytes());
        out.extend_from_slice(data);
        out
    }
}

/// Parses a four character reply code.
pub fn parse_reply(value: &str) -> Option<Reply> {
    Some(match value {
        "OKAY" => Reply::Okay,
        "FAIL" => Reply::Fail,
        "STAT" => Reply::Stat,
        "STA2" => Reply::Sta2,
        "LIST" => Reply::List,
        "DENT" => Reply::Dent,
        "RECV" => Reply::Recv,
        "DATA" => Reply::Data,
        "DONE" => Reply::Done,
        "SEND" => Reply::Send,
        "QUIT" => Reply::Quit,
        _ => return None,
    })
}

use crate::error::Error;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reply_codes() {
        assert_eq!(Protocol::OKAY, "OKAY");
        assert_eq!(Protocol::FAIL, "FAIL");
        assert_eq!(Reply::Okay.to_string(), "OKAY");
        assert_eq!(parse_reply("STA2"), Some(Reply::Sta2));
        assert_eq!(parse_reply("NOPE"), None);
    }

    #[test]
    fn test_decode_length() {
        assert_eq!(Protocol::decode_length("0046").unwrap(), 0x46);
        assert_eq!(Protocol::decode_length("5887").unwrap(), 0x5887);
        assert!(Protocol::decode_length("zzzz").is_err());
    }

    #[test]
    fn test_encode_length() {
        assert_eq!(Protocol::encode_length(27), "001B");
        assert_eq!(Protocol::encode_length(32), "0020");
        assert_eq!(Protocol::encode_length(9999), "270F");
        assert_eq!(Protocol::encode_length(0x0abc), "0ABC");
        assert_eq!(Protocol::encode_length(1).len(), 4);
        assert_eq!(Protocol::encode_length(57).len(), 4);
        assert_eq!(Protocol::encode_length(0), "0000");
    }

    #[test]
    fn test_encode_data() {
        let data = Protocol::encode_data(b"");
        assert_eq!(data, b"0000".to_vec());

        let data = Protocol::encode_data(vec![0u8; 0x270f]);
        assert_eq!(data.len(), 0x270f + 4);
        assert_eq!(&data[0..4], b"270F");
    }
}
