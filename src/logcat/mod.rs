//! Logcat support.
//!
//! Ported from the `@devicefarmer/adbkit-logcat` package (`lib/logcat/*.js`).

pub mod entry;
pub mod parser;
pub mod priority;
pub mod reader;
pub mod transform;

pub use entry::Entry;
pub use parser::Binary;
pub use priority::Priority;
pub use reader::{Filters, Logcat, Reader, ReaderEvent, ReaderFormat, ReaderOptions, ANY};
pub use transform::Transform;

/// The reader, re-exported for convenience.
pub use reader::Reader as LogcatReader;
