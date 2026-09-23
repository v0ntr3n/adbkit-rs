//! The parser for the `monkey` protocol.
//!
//! Ported from `lib/monkey/parser.js`.

use crate::error::{Error, Result};
use crate::monkey::reply::{Reply, ReplyType};

/// Parses the line-based replies of the `monkey` utility.
///
/// Ported from the `Parser` class of `lib/monkey/parser.js`.
#[derive(Debug, Default)]
pub struct Parser {
    buffer: Vec<u8>,
}

impl Parser {
    /// Creates a new parser.
    pub fn new() -> Self {
        Parser { buffer: Vec::new() }
    }

    /// Parses a chunk of data, returning every complete reply it contains.
    ///
    /// Ported from `Parser.parse()`. Incomplete lines are kept for the next
    /// chunk.
    pub fn parse(&mut self, chunk: &[u8]) -> Result<Vec<Reply>> {
        self.buffer.extend_from_slice(chunk);
        let mut replies = Vec::new();

        while let Some(index) = self.buffer.iter().position(|byte| *byte == 0x0a) {
            let line: Vec<u8> = self.buffer.drain(..=index).collect();
            let line = &line[..line.len() - 1];
            replies.push(Self::parse_line(line)?);
        }

        Ok(replies)
    }

    /// Parses a single line.
    ///
    /// Ported from `Parser._parseLine()`.
    fn parse_line(line: &[u8]) -> Result<Reply> {
        Ok(match line.first() {
            // 0x4f: 'O'
            Some(0x4f) => {
                if line.len() == 2 {
                    Reply::new(ReplyType::Ok, None)
                } else {
                    Reply::new(
                        ReplyType::Ok,
                        Some(String::from_utf8_lossy(&line[3..]).into_owned()),
                    )
                }
            }
            // 0x45: 'E'
            Some(0x45) => {
                if line.len() == 5 {
                    Reply::new(ReplyType::Error, None)
                } else {
                    Reply::new(
                        ReplyType::Error,
                        Some(String::from_utf8_lossy(&line[6..]).into_owned()),
                    )
                }
            }
            _ => {
                return Err(Error::Device(format!(
                    "Unparseable line '{}'",
                    String::from_utf8_lossy(line)
                )))
            }
        })
    }

    /// The bytes that are buffered while waiting for a complete line.
    pub fn buffered(&self) -> &[u8] {
        &self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ok_reply() {
        let mut parser = Parser::new();
        let replies = parser.parse(b"OK\n").unwrap();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].r#type, ReplyType::Ok);
        assert_eq!(replies[0].value, None);
    }

    #[test]
    fn test_error_reply() {
        let mut parser = Parser::new();
        let replies = parser.parse(b"ERROR Invalid command\n").unwrap();
        assert_eq!(replies.len(), 1);
        assert!(replies[0].is_error());
        assert_eq!(replies[0].value.as_deref(), Some("Invalid command"));
    }

    #[test]
    fn test_multiple_replies_in_one_chunk() {
        let mut parser = Parser::new();
        let replies = parser.parse(b"OK\nOK\nOK\n").unwrap();
        assert_eq!(replies.len(), 3);
    }

    #[test]
    fn test_split_chunks() {
        let mut parser = Parser::new();
        assert!(parser.parse(b"OK").unwrap().is_empty());
        assert_eq!(parser.buffered(), b"OK");
        let replies = parser.parse(b"\n").unwrap();
        assert_eq!(replies.len(), 1);
        assert!(parser.buffered().is_empty());
    }

    #[test]
    fn test_unparseable_line() {
        let mut parser = Parser::new();
        let err = parser.parse(b"nonsense\n").unwrap_err();
        assert_eq!(err.to_string(), "Unparseable line 'nonsense'");
    }
}
