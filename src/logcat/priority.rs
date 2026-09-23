//! Log priorities.
//!
//! Ported from `lib/logcat/priority.js`.

/// A log priority, as used by Android's logcat.
///
/// Ported from the `Priority` class of `lib/logcat/priority.js`. The default is
/// `Debug`, matching the reader's default priority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Priority used for miscellaneous logging.
    Unknown = 0,
    /// Priority used for default logging.
    Default = 1,
    /// Verbose logging.
    Verbose = 2,
    /// Debug logging.
    ///
    /// This is the default priority, matching the reader's default.
    #[default]
    Debug = 3,
    /// Informational logging.
    Info = 4,
    /// Warning logging.
    Warn = 5,
    /// Error logging.
    Error = 6,
    /// Fatal logging.
    Fatal = 7,
    /// Silent logging.
    Silent = 8,
}

impl Priority {
    /// All priorities, in numeric order.
    pub const ALL: [Priority; 9] = [
        Priority::Unknown,
        Priority::Default,
        Priority::Verbose,
        Priority::Debug,
        Priority::Info,
        Priority::Warn,
        Priority::Error,
        Priority::Fatal,
        Priority::Silent,
    ];

    /// Parses a priority by name, e.g. `debug` or `DEBUG`.
    ///
    /// Ported from `Priority.fromName()`.
    pub fn from_name(name: &str) -> Option<Priority> {
        match name.to_uppercase().as_str() {
            "UNKNOWN" => Some(Priority::Unknown),
            "DEFAULT" => Some(Priority::Default),
            "VERBOSE" => Some(Priority::Verbose),
            "DEBUG" => Some(Priority::Debug),
            "INFO" => Some(Priority::Info),
            "WARN" => Some(Priority::Warn),
            "ERROR" => Some(Priority::Error),
            "FATAL" => Some(Priority::Fatal),
            "SILENT" => Some(Priority::Silent),
            _ => Priority::from_letter(name),
        }
    }

    /// The name of the priority.
    ///
    /// Ported from `Priority.toName()`.
    pub fn name(&self) -> &'static str {
        match self {
            Priority::Unknown => "UNKNOWN",
            Priority::Default => "DEFAULT",
            Priority::Verbose => "VERBOSE",
            Priority::Debug => "DEBUG",
            Priority::Info => "INFO",
            Priority::Warn => "WARN",
            Priority::Error => "ERROR",
            Priority::Fatal => "FATAL",
            Priority::Silent => "SILENT",
        }
    }

    /// Parses a priority by letter, e.g. `D`.
    ///
    /// Ported from `Priority.fromLetter()`.
    pub fn from_letter(letter: &str) -> Option<Priority> {
        match letter.to_uppercase().as_str() {
            "?" => Some(Priority::Unknown),
            "V" => Some(Priority::Verbose),
            "D" => Some(Priority::Debug),
            "I" => Some(Priority::Info),
            "W" => Some(Priority::Warn),
            "E" => Some(Priority::Error),
            "F" => Some(Priority::Fatal),
            "S" => Some(Priority::Silent),
            _ => None,
        }
    }

    /// The letter of the priority.
    ///
    /// Ported from `Priority.toLetter()`.
    pub fn letter(&self) -> &'static str {
        match self {
            Priority::Unknown => "?",
            Priority::Default => "?",
            Priority::Verbose => "V",
            Priority::Debug => "D",
            Priority::Info => "I",
            Priority::Warn => "W",
            Priority::Error => "E",
            Priority::Fatal => "F",
            Priority::Silent => "S",
        }
    }

    /// The numeric value of the priority.
    pub fn value(&self) -> u8 {
        *self as u8
    }

    /// Parses a priority from its numeric value.
    pub fn from_value(value: u8) -> Priority {
        Priority::ALL
            .iter()
            .copied()
            .find(|priority| priority.value() == value)
            .unwrap_or(Priority::Unknown)
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl TryFrom<u8> for Priority {
    type Error = String;

    fn try_from(value: u8) -> std::result::Result<Self, <Self as TryFrom<u8>>::Error> {
        Priority::ALL
            .iter()
            .copied()
            .find(|priority| priority.value() == value)
            .ok_or_else(|| format!("Invalid priority {}", value))
    }
}

impl From<Priority> for u8 {
    fn from(priority: Priority) -> Self {
        priority.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_values() {
        assert_eq!(Priority::Unknown.value(), 0);
        assert_eq!(Priority::Default.value(), 1);
        assert_eq!(Priority::Verbose.value(), 2);
        assert_eq!(Priority::Silent.value(), 8);
    }

    #[test]
    fn test_from_name() {
        assert_eq!(Priority::from_name("DEBUG"), Some(Priority::Debug));
        assert_eq!(Priority::from_name("debug"), Some(Priority::Debug));
        // Falls back to letters.
        assert_eq!(Priority::from_name("I"), Some(Priority::Info));
        assert_eq!(Priority::from_name("?"), Some(Priority::Unknown));
        assert_eq!(Priority::from_name("nope"), None);
    }

    #[test]
    fn test_names_and_letters() {
        assert_eq!(Priority::Error.name(), "ERROR");
        assert_eq!(Priority::Error.letter(), "E");
        assert_eq!(Priority::Default.letter(), "?");
        assert_eq!(Priority::Silent.name(), "SILENT");
        assert_eq!(Priority::Info.to_string(), "INFO");
    }

    #[test]
    fn test_from_value() {
        assert_eq!(Priority::from_value(4), Priority::Info);
        assert_eq!(Priority::from_value(200), Priority::Unknown);
    }
}
