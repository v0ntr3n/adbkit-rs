//! The parser for the Android binary log format.
//!
//! Ported from `lib/logcat/parser/binary.js`.

use crate::error::{Error, Result};
use crate::logcat::entry::Entry;

/// The header size used by v1 of the binary format.
const HEADER_SIZE_V1: usize = 20;
/// The maximum plausible header size.
const HEADER_SIZE_MAX: usize = 100;

/// Parses the binary log format used by `logcat -B`.
///
/// Ported from the `Binary` class of `lib/logcat/parser/binary.js`.
#[derive(Debug, Default)]
pub struct Binary {
    buffer: Vec<u8>,
}

impl Binary {
    /// Creates a new parser.
    pub fn new() -> Self {
        Binary { buffer: Vec::new() }
    }

    /// Parses a chunk of data, returning every complete entry it contains.
    ///
    /// Ported from `Binary.parse()`.
    pub fn parse(&mut self, chunk: &[u8]) -> Result<Vec<Entry>> {
        self.buffer.extend_from_slice(chunk);
        let mut entries = Vec::new();

        while self.buffer.len() > 4 {
            let length = u16::from_le_bytes([self.buffer[0], self.buffer[1]]) as usize;
            // The header size is read for validation only; the payload always
            // starts after it.
            let header_size = u16::from_le_bytes([self.buffer[2], self.buffer[3]]) as usize;
            let header_size = if !(HEADER_SIZE_V1..=HEADER_SIZE_MAX).contains(&header_size) {
                HEADER_SIZE_V1
            } else {
                header_size
            };

            if self.buffer.len() < header_size + length {
                break;
            }

            let mut entry = Entry::new();
            let pid = i32::from_le_bytes(self.read::<4>(4));
            let tid = i32::from_le_bytes(self.read::<4>(8));
            let sec = i32::from_le_bytes(self.read::<4>(12));
            let nsec = i32::from_le_bytes(self.read::<4>(16));
            entry.set_pid(pid);
            entry.set_tid(tid);
            entry.set_date(crate::logcat::entry::Entry::date_from_millis(
                sec as i64 * 1000 + nsec as i64 / 1_000_000,
            ));

            // Make sure that we don't choke if new fields are added.
            let data = self.buffer[header_size..header_size + length].to_vec();
            self.buffer.drain(..header_size + length);

            Self::process_entry(&mut entry, &data)?;
            entries.push(entry);
        }

        Ok(entries)
    }

    /// The bytes that are buffered while waiting for a complete entry.
    pub fn buffered(&self) -> &[u8] {
        &self.buffer
    }

    fn read<const N: usize>(&self, offset: usize) -> [u8; N] {
        let mut bytes = [0u8; N];
        bytes.copy_from_slice(&self.buffer[offset..offset + N]);
        bytes
    }

    /// Splits the payload of an entry into priority, tag and message.
    ///
    /// Ported from `Binary._processEntry()`.
    fn process_entry(entry: &mut Entry, data: &[u8]) -> Result<()> {
        entry.set_priority_value(*data.first().ok_or_else(|| {
            Error::Device(format!(
                "Unprocessable entry data '{}'",
                String::from_utf8_lossy(data)
            ))
        })?);

        for cursor in 1..data.len() {
            if data[cursor] == 0 {
                entry.set_tag(String::from_utf8_lossy(&data[1..cursor]).into_owned());
                entry.set_message(
                    String::from_utf8_lossy(&data[cursor + 1..data.len().saturating_sub(1)])
                        .into_owned(),
                );
                return Ok(());
            }
        }

        Err(Error::Device(format!(
            "Unprocessable entry data '{}'",
            String::from_utf8_lossy(data)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logcat::entry::HEADER_SIZE;
    use crate::logcat::priority::Priority;
    use std::time::UNIX_EPOCH;

    fn entry_bytes(
        pid: i32,
        tid: i32,
        secs: i64,
        priority: Priority,
        tag: &str,
        message: &str,
    ) -> Vec<u8> {
        let mut buffer = Vec::new();
        let payload_len = 1 + tag.len() + 1 + message.len() + 1;
        let length = payload_len as u16;
        buffer.extend_from_slice(&length.to_le_bytes());
        buffer.extend_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        buffer.extend_from_slice(&pid.to_le_bytes());
        buffer.extend_from_slice(&tid.to_le_bytes());
        buffer.extend_from_slice(&(secs as i32).to_le_bytes());
        buffer.extend_from_slice(&0i32.to_le_bytes());
        buffer.push(priority.value());
        buffer.extend_from_slice(tag.as_bytes());
        buffer.push(0);
        buffer.extend_from_slice(message.as_bytes());
        buffer.push(0);
        buffer
    }

    #[test]
    fn test_parses_an_entry() {
        let mut parser = Binary::new();
        let entries = parser
            .parse(&entry_bytes(
                1234,
                5678,
                1000,
                Priority::Info,
                "tag",
                "hello",
            ))
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pid, 1234);
        assert_eq!(entries[0].tid, 5678);
        assert_eq!(entries[0].priority, Priority::Info);
        assert_eq!(entries[0].tag.as_deref(), Some("tag"));
        assert_eq!(entries[0].message.as_deref(), Some("hello"));
        assert_eq!(
            entries[0].date,
            Some(UNIX_EPOCH + std::time::Duration::from_secs(1000))
        );
        assert!(parser.buffered().is_empty());
    }

    #[test]
    fn test_waits_for_complete_entries() {
        let mut parser = Binary::new();
        let bytes = entry_bytes(1, 2, 3, Priority::Debug, "t", "m");
        assert!(parser.parse(&bytes[..bytes.len() - 3]).unwrap().is_empty());
        assert!(!parser.buffered().is_empty());
        let entries = parser.parse(&bytes[bytes.len() - 3..]).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message.as_deref(), Some("m"));
    }

    #[test]
    fn test_parses_multiple_entries_in_one_chunk() {
        let mut parser = Binary::new();
        let mut bytes = entry_bytes(1, 2, 3, Priority::Debug, "a", "1");
        bytes.extend(entry_bytes(4, 5, 6, Priority::Warn, "b", "2"));
        let entries = parser.parse(&bytes).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].priority, Priority::Warn);
        assert_eq!(entries[1].tag.as_deref(), Some("b"));
    }

    #[test]
    fn test_rejects_unprocessable_data() {
        let mut parser = Binary::new();
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&5u16.to_le_bytes());
        buffer.extend_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        buffer.extend_from_slice(&1i32.to_le_bytes());
        buffer.extend_from_slice(&2i32.to_le_bytes());
        buffer.extend_from_slice(&3i32.to_le_bytes());
        buffer.extend_from_slice(&4i32.to_le_bytes());
        // No NUL byte in the payload at all.
        buffer.extend_from_slice(&[Priority::Info.value(), b'a', b'b', b'c', b'd']);
        let err = parser.parse(&buffer).unwrap_err();
        assert!(err.to_string().starts_with("Unprocessable entry data"));
    }

    #[test]
    fn test_bad_header_size_is_ignored() {
        // Some devices report a nonsense header size; the parser has to fall
        // back to 20.
        let mut bytes = entry_bytes(1, 2, 3, Priority::Info, "t", "m");
        bytes[2..4].copy_from_slice(&0u16.to_le_bytes());
        let mut parser = Binary::new();
        let entries = parser.parse(&bytes).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tag.as_deref(), Some("t"));
    }
}
