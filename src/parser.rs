//! A reader for the ADB protocol.
//!
//! Ported from `src/adb/parser.ts`. The TypeScript original hooks into Node's
//! `readable`/`end`/`error`/`timeout` stream events; this implementation wraps
//! a [`BufReader`] instead.

use std::time::Duration;

use regex::Regex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, BufReader};

use crate::error::{Error, Result};
use crate::protocol::{parse_reply, Protocol, Reply};

/// Reads protocol data from an ADB connection.
pub struct Parser<R> {
    reader: BufReader<R>,
    ended: bool,
    timeout: Option<Duration>,
}

impl<R> Parser<R>
where
    R: AsyncRead + Unpin,
{
    /// Creates a new parser for the given stream.
    ///
    /// `timeout` mirrors Node's socket timeout: `None` means no timeout.
    pub fn new(stream: R, timeout: Option<Duration>) -> Self {
        Parser {
            reader: BufReader::with_capacity(64 * 1024, stream),
            ended: false,
            timeout,
        }
    }

    /// The configured socket timeout, if any.
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Reads exactly `how_many` bytes.
    ///
    /// Mirrors `Parser.readBytes()`. Rejects with [`Error::PrematureEof`] if
    /// the stream ends first.
    pub async fn read_bytes(&mut self, how_many: usize) -> Result<Vec<u8>> {
        if how_many == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; how_many];
        let read = self.read_some(&mut buf).await?;
        buf.truncate(read);
        if read < how_many {
            return Err(Error::PrematureEof(how_many - read));
        }
        Ok(buf)
    }

    /// Reads `how_many` bytes and decodes them as ASCII.
    ///
    /// Mirrors `Parser.readAscii()`.
    pub async fn read_ascii(&mut self, how_many: usize) -> Result<String> {
        let chunk = self.read_bytes(how_many).await?;
        Ok(String::from_utf8_lossy(&chunk).into_owned())
    }

    /// Reads a length-prefixed value.
    ///
    /// Mirrors `Parser.readValue()`.
    pub async fn read_value(&mut self) -> Result<Vec<u8>> {
        let length = self.read_ascii(4).await?;
        let length = Protocol::decode_length(&length)? as usize;
        self.read_bytes(length).await
    }

    /// Reads until the given byte code (inclusive is *not* returned).
    ///
    /// Mirrors `Parser.readUntil()`.
    pub async fn read_until(&mut self, code: u8) -> Result<Vec<u8>> {
        let mut skipped = Vec::new();
        loop {
            let chunk = self.read_bytes(1).await?;
            if chunk[0] == code {
                return Ok(skipped);
            }
            skipped.extend_from_slice(&chunk);
        }
    }

    /// Reads a single line, stripping a trailing `\r`.
    ///
    /// Mirrors `Parser.readLine()`.
    pub async fn read_line(&mut self) -> Result<Vec<u8>> {
        let mut line = self.read_until(0x0a).await?;
        if line.last() == Some(&0x0d) {
            line.pop();
        }
        Ok(line)
    }

    /// Reads lines until one matches the given regular expression.
    ///
    /// Mirrors `Parser.searchLine()`. Returns the capture groups of the
    /// matching line; index 0 is always the full match.
    pub async fn search_line(&mut self, re: &Regex) -> Result<Vec<Option<String>>> {
        let timeout = self.timeout;
        self.search_line_with_timeout(re, timeout).await
    }

    /// Like [`Parser::search_line`], but with an explicit timeout.
    ///
    /// The `monkey` command uses a 1 second timeout and treats a timeout as
    /// success (some devices never print the banner).
    pub async fn search_line_with_timeout(
        &mut self,
        re: &Regex,
        timeout: Option<Duration>,
    ) -> Result<Vec<Option<String>>> {
        match timeout {
            Some(timeout) if !timeout.is_zero() => {
                match tokio::time::timeout(timeout, self.search_line_inner(re)).await {
                    Ok(result) => result,
                    Err(_) => Err(Error::Timeout),
                }
            }
            _ => self.search_line_inner(re).await,
        }
    }

    async fn search_line_inner(&mut self, re: &Regex) -> Result<Vec<Option<String>>> {
        loop {
            let line = self.read_line().await?;
            let line = String::from_utf8_lossy(&line).into_owned();
            if let Some(captures) = re.captures(&line) {
                let mut groups = Vec::with_capacity(captures.len());
                for index in 0..captures.len() {
                    groups.push(captures.get(index).map(|m| m.as_str().to_string()));
                }
                return Ok(groups);
            }
        }
    }

    /// Reads everything until the stream ends.
    ///
    /// Mirrors `Parser.readAll()`.
    pub async fn read_all(&mut self) -> Result<Vec<u8>> {
        let mut all = Vec::new();
        self.reader
            .read_to_end(&mut all)
            .await
            .map_err(Error::from)?;
        Ok(all)
    }

    /// Reads `how_many` bytes and writes them to `target`.
    ///
    /// Mirrors `Parser.readByteFlow()`.
    pub async fn read_byte_flow<W>(&mut self, how_many: usize, target: &mut W) -> Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        if how_many == 0 {
            return Ok(());
        }
        let mut limited = (&mut self.reader).take(how_many as u64);
        let copied = tokio::io::copy(&mut limited, target)
            .await
            .map_err(Error::from)?;
        if copied < how_many as u64 {
            return Err(Error::PrematureEof(how_many - copied as usize));
        }
        Ok(())
    }

    /// Reads a `FAIL` reply and turns it into an error.
    ///
    /// Mirrors `Parser.readError()`.
    pub async fn read_error(&mut self) -> Error {
        match self.read_value().await {
            Ok(value) => Error::Failure(String::from_utf8_lossy(&value).into_owned()),
            Err(err) => err,
        }
    }

    /// Rejects with an [`Error::Unexpected`] error.
    ///
    /// Mirrors `Parser.unexpected()`.
    pub fn unexpected<T, U: Into<String>, E: Into<String>>(
        &mut self,
        unexpected: U,
        expected: E,
    ) -> Result<T> {
        Err(Error::unexpected(unexpected, expected))
    }

    /// Reads everything that is left on the stream and discards it.
    ///
    /// Mirrors `Parser.end()`.
    pub async fn end(&mut self) -> Result<bool> {
        if self.ended {
            return Ok(true);
        }
        let _ = self.read_all().await;
        self.ended = true;
        Ok(true)
    }

    /// Whether [`Parser::end`] has been called.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Consumes the parser, returning the underlying stream together with any
    /// bytes that were buffered but not yet consumed.
    ///
    /// Used to hand the connection over to the caller after commands such as
    /// `shell:`, where the parser must not lose buffered payload data.
    pub fn into_parts(self) -> (R, Vec<u8>) {
        let pending = self.reader.buffer().to_vec();
        (self.reader.into_inner(), pending)
    }

    async fn read_some(&mut self, buf: &mut [u8]) -> Result<usize> {
        match self.timeout {
            Some(timeout) if !timeout.is_zero() => {
                match tokio::time::timeout(timeout, self.reader.read(buf)).await {
                    Ok(result) => result.map_err(Error::from),
                    Err(_) => Err(Error::Timeout),
                }
            }
            _ => self.reader.read(buf).await.map_err(Error::from),
        }
    }
}

impl<R> tokio::io::AsyncRead for Parser<R>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl<R> std::fmt::Debug for Parser<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parser")
            .field("ended", &self.ended)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

/// Reads a four character reply code and dispatches on it.
///
/// This is the shape shared by virtually every command in the TypeScript
/// implementation:
///
/// ```typescript
/// return this.parser.readAscii(4).then((reply) => {
///     switch (reply) {
///         case Protocol.OKAY: /* ... */
///         case Protocol.FAIL: return this.parser.readError();
///         default: return this.parser.unexpected(reply, 'OKAY or FAIL');
///     }
/// });
/// ```
pub async fn read_reply<R>(parser: &mut Parser<R>) -> Result<Reply>
where
    R: AsyncRead + Unpin,
{
    let reply = parser.read_ascii(4).await?;
    match parse_reply(&reply) {
        Some(Reply::Okay) | Some(Reply::Stat) | Some(Reply::Sta2) | Some(Reply::List)
        | Some(Reply::Dent) | Some(Reply::Recv) | Some(Reply::Data) | Some(Reply::Done)
        | Some(Reply::Send) | Some(Reply::Quit) => Ok(parse_reply(&reply).unwrap()),
        Some(Reply::Fail) => Err(parser.read_error().await),
        None => parser.unexpected(reply, "a valid reply code"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_bytes() {
        let mut parser = Parser::new(&b"hello world"[..], None);
        assert_eq!(parser.read_bytes(5).await.unwrap(), b"hello".to_vec());
        assert_eq!(parser.read_ascii(1).await.unwrap(), " ");
        assert_eq!(parser.read_bytes(5).await.unwrap(), b"world".to_vec());
        assert_eq!(
            parser.read_bytes(1).await.unwrap_err(),
            Error::PrematureEof(1)
        );
    }

    #[tokio::test]
    async fn test_read_zero_bytes() {
        let mut parser = Parser::new(&b""[..], None);
        assert_eq!(parser.read_bytes(0).await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn test_read_ascii() {
        let mut parser = Parser::new(&b"OKAY"[..], None);
        assert_eq!(parser.read_ascii(4).await.unwrap(), "OKAY");
    }

    #[tokio::test]
    async fn test_read_value() {
        let data = Protocol::encode_data("foobar");
        let mut parser = Parser::new(&data[..], None);
        assert_eq!(parser.read_value().await.unwrap(), b"foobar".to_vec());
    }

    #[tokio::test]
    async fn test_read_line() {
        let mut parser = Parser::new(&b"first\r\nsecond\nthird"[..], None);
        assert_eq!(parser.read_line().await.unwrap(), b"first".to_vec());
        assert_eq!(parser.read_line().await.unwrap(), b"second".to_vec());
        assert_eq!(
            parser.read_line().await.unwrap_err(),
            Error::PrematureEof(1)
        );
    }

    #[tokio::test]
    async fn test_read_until() {
        let mut parser = Parser::new(&b"ab\x00cd\x00"[..], None);
        assert_eq!(parser.read_until(0).await.unwrap(), b"ab".to_vec());
        assert_eq!(parser.read_until(0).await.unwrap(), b"cd".to_vec());
    }

    #[tokio::test]
    async fn test_read_until_eof() {
        let mut parser = Parser::new(&b"ab"[..], None);
        assert_eq!(
            parser.read_until(0).await.unwrap_err(),
            Error::PrematureEof(1)
        );
    }

    #[tokio::test]
    async fn test_search_line() {
        // Ported from the `searchLine(re)` tests of `test/adb/parser.ts`.
        let re = Regex::new(r"za(p)").unwrap();
        let mut parser = Parser::new(&b"foo bar\nzip zap\npip pop\n"[..], None);
        let groups = parser.search_line(&re).await.unwrap();
        assert_eq!(groups[0].as_deref(), Some("zap"));
        assert_eq!(groups[1].as_deref(), Some("p"));
    }

    #[tokio::test]
    async fn test_search_line_without_a_match() {
        let re = Regex::new(r"nope").unwrap();
        // The stream ends before a line is found.
        let mut parser = Parser::new(&b"foo bar"[..], None);
        assert_eq!(
            parser.search_line(&re).await.unwrap_err(),
            Error::PrematureEof(1)
        );
    }

    #[tokio::test]
    async fn test_read_all() {
        let mut parser = Parser::new(&b"one two three"[..], None);
        assert_eq!(parser.read_all().await.unwrap(), b"one two three".to_vec());
    }

    #[tokio::test]
    async fn test_read_byte_flow() {
        let mut parser = Parser::new(&b"0123456789"[..], None);
        let mut out = Vec::new();
        parser.read_byte_flow(4, &mut out).await.unwrap();
        assert_eq!(out, b"0123".to_vec());
        assert_eq!(parser.read_ascii(6).await.unwrap(), "456789");
    }

    #[tokio::test]
    async fn test_read_error() {
        let data = Protocol::encode_data("no such device");
        let mut parser = Parser::new(&data[..], None);
        assert_eq!(
            parser.read_error().await,
            Error::Failure("no such device".into())
        );
    }

    #[tokio::test]
    async fn test_end_is_idempotent() {
        let mut parser = Parser::new(&b"leftovers"[..], None);
        assert!(parser.end().await.unwrap());
        assert!(parser.is_ended());
        assert!(parser.end().await.unwrap());
    }

    #[tokio::test]
    async fn test_read_reply() {
        let mut parser = Parser::new(&b"OKAY"[..], None);
        assert_eq!(read_reply(&mut parser).await.unwrap(), Reply::Okay);
    }

    #[tokio::test]
    async fn test_read_reply_fail() {
        let mut data = b"FAIL".to_vec();
        data.extend_from_slice(&Protocol::encode_data("because"));
        let mut parser = Parser::new(&data[..], None);
        assert_eq!(
            read_reply(&mut parser).await.unwrap_err(),
            Error::Failure("because".into())
        );
    }

    #[tokio::test]
    async fn test_read_reply_unexpected() {
        let mut parser = Parser::new(&b"WXYZ"[..], None);
        assert_eq!(
            read_reply(&mut parser).await.unwrap_err(),
            Error::unexpected("WXYZ", "a valid reply code")
        );
    }

    #[tokio::test]
    async fn test_search_line_timeout() {
        // A stream that never sends a matching line and never ends.
        let (client, _server) = tokio::io::duplex(1024);
        let re = Regex::new(r"never matches").unwrap();
        let mut parser = Parser::new(client, None);
        let err = parser
            .search_line_with_timeout(&re, Some(Duration::from_millis(50)))
            .await
            .unwrap_err();
        assert_eq!(err, Error::Timeout);
    }
}
