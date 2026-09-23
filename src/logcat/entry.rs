//! A single logcat entry.
//!
//! Ported from `lib/logcat/entry.js`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::logcat::priority::Priority;

/// The size of the binary logcat header.
pub const HEADER_SIZE: usize = 20;

/// A single entry of the Android binary log.
///
/// Ported from the `Entry` class of `lib/logcat/entry.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The date of the entry.
    pub date: Option<SystemTime>,
    /// The process id.
    pub pid: i32,
    /// The thread id.
    pub tid: i32,
    /// The priority.
    pub priority: Priority,
    /// The tag.
    pub tag: Option<String>,
    /// The message.
    pub message: Option<String>,
}

impl Default for Entry {
    fn default() -> Self {
        Entry {
            date: None,
            pid: -1,
            tid: -1,
            priority: Priority::Unknown,
            tag: None,
            message: None,
        }
    }
}

impl Entry {
    /// Creates a new, empty entry.
    pub fn new() -> Self {
        Entry::default()
    }

    /// Sets the date of the entry.
    pub fn set_date(&mut self, date: SystemTime) -> &mut Self {
        self.date = Some(date);
        self
    }

    /// Sets the process id.
    pub fn set_pid(&mut self, pid: i32) -> &mut Self {
        self.pid = pid;
        self
    }

    /// Sets the thread id.
    pub fn set_tid(&mut self, tid: i32) -> &mut Self {
        self.tid = tid;
        self
    }

    /// Sets the priority.
    pub fn set_priority(&mut self, priority: Priority) -> &mut Self {
        self.priority = priority;
        self
    }

    /// Sets the priority from its raw numeric value.
    pub fn set_priority_value(&mut self, priority: u8) -> &mut Self {
        self.priority = Priority::from_value(priority);
        self
    }

    /// Sets the tag.
    pub fn set_tag(&mut self, tag: impl Into<String>) -> &mut Self {
        self.tag = Some(tag.into());
        self
    }

    /// Sets the message.
    pub fn set_message(&mut self, message: impl Into<String>) -> &mut Self {
        self.message = Some(message.into());
        self
    }

    /// Encodes the entry in the binary logcat format.
    ///
    /// Ported from `Entry.toBinary()`.
    pub fn to_binary(&self) -> Vec<u8> {
        let tag = self.tag.as_deref().unwrap_or("");
        let message = self.message.as_deref().unwrap_or("");

        let mut length = HEADER_SIZE; // header
        length += 1; // priority
        length += tag.len();
        length += 1; // NULL-byte
        length += message.len();
        length += 1; // NULL-byte

        let mut buffer = vec![0u8; length];
        let mut cursor = 0usize;
        buffer[0..2].copy_from_slice(&((length - HEADER_SIZE) as u16).to_le_bytes());
        cursor += 4; // include 2 bytes of padding
        buffer[cursor..cursor + 4].copy_from_slice(&self.pid.to_le_bytes());
        cursor += 4;
        buffer[cursor..cursor + 4].copy_from_slice(&self.tid.to_le_bytes());
        cursor += 4;
        let millis = self
            .date
            .and_then(|date| date.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        buffer[cursor..cursor + 4].copy_from_slice(&((millis / 1000) as i32).to_le_bytes());
        cursor += 4;
        buffer[cursor..cursor + 4]
            .copy_from_slice(&(((millis % 1000) * 1_000_000) as i32).to_le_bytes());
        cursor += 4;
        buffer[cursor] = self.priority.value();
        cursor += 1;
        buffer[cursor..cursor + tag.len()].copy_from_slice(tag.as_bytes());
        cursor += tag.len();
        buffer[cursor] = 0x00;
        cursor += 1;
        buffer[cursor..cursor + message.len()].copy_from_slice(message.as_bytes());
        cursor += message.len();
        buffer[cursor] = 0x00;
        buffer
    }

    /// The date of the entry, if any.
    pub fn date_from_millis(millis: i64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(millis.max(0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let entry = Entry::new();
        assert_eq!(entry.pid, -1);
        assert_eq!(entry.tid, -1);
        assert_eq!(entry.priority, Priority::Unknown);
        assert_eq!(entry.tag, None);
        assert_eq!(entry.message, None);
    }

    #[test]
    fn test_to_binary() {
        let mut entry = Entry::new();
        entry.set_pid(1234);
        entry.set_tid(5678);
        entry.set_date(UNIX_EPOCH + Duration::from_secs(1000));
        entry.set_priority(Priority::Info);
        entry.set_tag("tag");
        entry.set_message("hello");

        let binary = entry.to_binary();
        let expected_length = HEADER_SIZE + 1 + 3 + 1 + 5 + 1;
        assert_eq!(binary.len(), expected_length);
        assert_eq!(
            u16::from_le_bytes([binary[0], binary[1]]) as usize,
            expected_length - HEADER_SIZE
        );
        assert_eq!(i32::from_le_bytes(binary[4..8].try_into().unwrap()), 1234);
        assert_eq!(i32::from_le_bytes(binary[8..12].try_into().unwrap()), 5678);
        assert_eq!(i32::from_le_bytes(binary[12..16].try_into().unwrap()), 1000);
        assert_eq!(i32::from_le_bytes(binary[16..20].try_into().unwrap()), 0);
        assert_eq!(binary[20], Priority::Info.value());
        assert_eq!(&binary[21..24], b"tag");
        assert_eq!(binary[24], 0);
        assert_eq!(&binary[25..30], b"hello");
        assert_eq!(binary[30], 0);
    }
}
