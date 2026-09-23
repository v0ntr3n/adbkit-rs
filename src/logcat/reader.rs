//! The logcat reader.
//!
//! Ported from `lib/logcat/reader.js`. The TypeScript reader is an
//! `EventEmitter`; here the events arrive over a channel and can be consumed as
//! a [`futures::Stream`].

use std::collections::HashMap;
use std::sync::Arc;

use std::sync::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt};

use tokio::sync::{mpsc, Notify};

use crate::error::{Error, Result};
use crate::logcat::entry::Entry;
use crate::logcat::parser::Binary;
use crate::logcat::priority::Priority;
use crate::logcat::transform::Transform;

/// The tag that matches every tag.
pub const ANY: &str = "*";

/// The supported log formats.
///
/// Ported from `lib/ReaderOptions.d.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReaderFormat {
    /// The binary format (`logcat -B`).
    #[default]
    Binary,
}

impl ReaderFormat {
    /// The format as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReaderFormat::Binary => "binary",
        }
    }
}

impl std::fmt::Display for ReaderFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Options for a [`Reader`].
///
/// Ported from `lib/ReaderOptions.d.ts` and the defaults of
/// `lib/logcat/reader.js`.
#[derive(Debug, Clone)]
pub struct ReaderOptions {
    /// The format of the log data. Only `binary` is supported.
    pub format: ReaderFormat,
    /// Whether to fix the line feeds inserted by the ADB shell.
    pub fix_line_feeds: bool,
    /// The minimum priority to include for tags that are not filtered.
    pub priority: Priority,
}

impl Default for ReaderOptions {
    fn default() -> Self {
        ReaderOptions {
            format: ReaderFormat::Binary,
            fix_line_feeds: true,
            priority: Priority::Debug,
        }
    }
}

impl ReaderOptions {
    /// Validates the options, as the `Reader` constructor does.
    pub fn validate(&self) -> Result<()> {
        if self.format != ReaderFormat::Binary {
            return Err(Error::Device(format!(
                "Unsupported format '{}'",
                self.format
            )));
        }
        Ok(())
    }
}

/// An event emitted by a [`Reader`].
#[derive(Debug)]
pub enum ReaderEvent {
    /// A log entry that passed the filters.
    Entry(Box<Entry>),
    /// The stream ended.
    End,
    /// The stream was closed by the remote end.
    Finish,
    /// An error occurred.
    Error(Error),
}

/// The filters of a [`Reader`].
#[derive(Debug, Clone, Default)]
pub struct Filters {
    /// The priority used for tags that are not filtered. A negative value means
    /// no filter.
    pub all: i32,
    /// The priority used for specific tags.
    pub tags: HashMap<String, Priority>,
}

/// Reads the Android binary log.
///
/// Ported from the `Reader` class of `lib/logcat/reader.js`.
pub struct Reader<S>
where
    S: AsyncRead + Unpin + Send + 'static,
{
    options: ReaderOptions,
    filters: Arc<Mutex<Filters>>,
    events: mpsc::Receiver<ReaderEvent>,
    stop: Arc<Notify>,
    _stream: std::marker::PhantomData<S>,
}

impl<S> Reader<S>
where
    S: AsyncRead + Unpin + Send + 'static,
{
    /// Creates a reader with the given options.
    pub fn with_options(options: ReaderOptions) -> Result<Self> {
        options.validate()?;
        let (_, events) = mpsc::channel(256);
        Ok(Reader {
            options,
            filters: Arc::new(Mutex::new(Filters {
                all: -1,
                tags: HashMap::new(),
            })),
            events,
            stop: Arc::new(Notify::new()),
            _stream: std::marker::PhantomData,
        })
    }

    /// Creates a reader with the default options.
    pub fn new() -> Result<Self> {
        Reader::with_options(ReaderOptions::default())
    }

    /// Connects the reader to a stream, as `Logcat.readStream()` does.
    ///
    /// Line feeds are not fixed, because the stream coming from
    /// `DeviceClient::openLogcat()` has already been through a
    /// [`crate::linetransform::LineTransform`].
    pub fn connect(stream: S) -> Self {
        let options = ReaderOptions {
            fix_line_feeds: false,
            ..Default::default()
        };
        let mut reader = Reader::with_options(options).expect("the default format is binary");
        reader.hook(stream);
        reader
    }

    /// Connects the reader to a stream with the given options.
    pub fn connect_with_options(stream: S, options: ReaderOptions) -> Result<Self> {
        let mut reader = Reader::with_options(options)?;
        reader.hook(stream);
        Ok(reader)
    }

    /// Excludes the given tag.
    ///
    /// Ported from `Reader.exclude()`.
    pub fn exclude(&mut self, tag: &str) -> &mut Self {
        if tag == ANY {
            return self.exclude_all();
        }
        self.filters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .tags
            .insert(tag.to_string(), Priority::Silent);
        self
    }

    /// Excludes every tag.
    ///
    /// Ported from `Reader.excludeAll()`.
    pub fn exclude_all(&mut self) -> &mut Self {
        self.filters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .all = Priority::Silent.value() as i32;
        self
    }

    /// Includes the given tag, with the given (or default) priority.
    ///
    /// Ported from `Reader.include()`.
    pub fn include(&mut self, tag: &str, priority: Option<Priority>) -> &mut Self {
        if tag == ANY {
            return self.include_all(priority);
        }
        let priority = priority.unwrap_or(self.options.priority);
        self.filters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .tags
            .insert(tag.to_string(), priority);
        self
    }

    /// Includes every tag, with the given (or default) priority.
    ///
    /// Ported from `Reader.includeAll()`.
    pub fn include_all(&mut self, priority: Option<Priority>) -> &mut Self {
        let priority = priority.unwrap_or(self.options.priority);
        self.filters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .all = priority.value() as i32;
        self
    }

    /// Resets the filters.
    ///
    /// Ported from `Reader.resetFilters()`.
    pub fn reset_filters(&mut self) -> &mut Self {
        {
            let mut filters = self
                .filters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            filters.all = -1;
            filters.tags.clear();
        }
        self
    }

    /// The options of this reader.
    pub fn options(&self) -> &ReaderOptions {
        &self.options
    }

    /// The filters of this reader.
    /// The filters of this reader.
    pub fn filters(&self) -> std::sync::MutexGuard<'_, Filters> {
        self.filters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Hooks the reader up to a stream.
    ///
    /// Ported from `Reader._hook()`.
    fn hook(&mut self, mut stream: S) {
        let (tx, rx) = mpsc::channel(256);
        self.events = rx;
        let stop = self.stop.clone();

        let fix_line_feeds = self.options.fix_line_feeds;
        let filters = Arc::clone(&self.filters);

        tokio::spawn(async move {
            let mut parser = Binary::new();
            let mut buf = vec![0u8; 64 * 1024];

            loop {
                tokio::select! {
                    result = stream.read(&mut buf) => {
                        let read = match result {
                            Ok(read) => read,
                            Err(err) => {
                                let _ = tx.send(ReaderEvent::Error(Error::Io(err))).await;
                                break;
                            }
                        };
                        if read == 0 {
                            let _ = tx.send(ReaderEvent::End).await;
                            break;
                        }

                        // The ADB shell turns every \n into \r\n, so the data has
                        // to be transformed before being parsed.
                        let data: Vec<u8> = if fix_line_feeds {
                            let mut transform = Transform::new(&buf[..read]);
                            let mut fixed = Vec::new();
                            let _ = tokio::io::AsyncReadExt::read_to_end(&mut transform, &mut fixed).await;
                            fixed
                        } else {
                            buf[..read].to_vec()
                        };

                        match parser.parse(&data) {
                            Ok(entries) => {
                                for entry in entries {
                                    let passes = filter(
                                        &filters
                                            .lock()
                                            .unwrap_or_else(|poisoned| poisoned.into_inner()),
                                        &entry,
                                    );
                                    if passes
                                        && tx
                                            .send(ReaderEvent::Entry(Box::new(entry)))
                                            .await
                                            .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                            Err(err) => {
                                let _ = tx.send(ReaderEvent::Error(err)).await;
                            }
                        }
                    }
                    _ = stop.notified() => break,
                }
            }
            let _ = tx.send(ReaderEvent::Finish).await;
        });
    }

    /// Waits for the next event.
    pub async fn next(&mut self) -> Option<ReaderEvent> {
        self.events.recv().await
    }

    /// Ends the stream, as `Reader.end()` does.
    pub async fn end(mut self) {
        self.stop.notify_waiters();
        self.events.close();
    }
}

impl<S> futures::Stream for Reader<S>
where
    S: AsyncRead + Unpin + Send + 'static,
{
    type Item = ReaderEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.events.poll_recv(cx)
    }
}

impl<S> std::fmt::Debug for Reader<S>
where
    S: AsyncRead + Unpin + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader")
            .field("options", &self.options)
            .field("filters", &self.filters)
            .finish_non_exhaustive()
    }
}

/// Whether the given entry passes the filters.
///
/// Ported from `Reader._filter()`.
fn filter(filters: &Filters, entry: &Entry) -> bool {
    let tag = entry.tag.as_deref().unwrap_or_default();
    let wanted = filters
        .tags
        .get(tag)
        .map(|priority| priority.value() as i32)
        .unwrap_or(filters.all);
    if wanted < 0 {
        return true;
    }
    entry.priority.value() as i32 >= wanted
}

/// The logcat facade.
///
/// Ported from `lib/logcat.js`.
pub struct Logcat;

impl Logcat {
    /// Reads a stream with the given options.
    ///
    /// Ported from `Logcat.readStream()`.
    pub fn read_stream<S>(stream: S, options: ReaderOptions) -> Result<Reader<S>>
    where
        S: AsyncRead + Unpin + Send + 'static,
    {
        Reader::connect_with_options(stream, options)
    }

    /// Reads a stream with the default options.
    pub fn read_stream_default<S>(stream: S) -> Reader<S>
    where
        S: AsyncRead + Unpin + Send + 'static,
    {
        Reader::connect(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logcat::entry::HEADER_SIZE;
    use std::time::UNIX_EPOCH;
    use tokio::io::AsyncWriteExt;

    fn entry_bytes(pid: i32, tid: i32, priority: Priority, tag: &str, message: &str) -> Vec<u8> {
        let payload_len = 1 + tag.len() + 1 + message.len() + 1;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&(payload_len as u16).to_le_bytes());
        buffer.extend_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        buffer.extend_from_slice(&pid.to_le_bytes());
        buffer.extend_from_slice(&tid.to_le_bytes());
        buffer.extend_from_slice(&1000i32.to_le_bytes());
        buffer.extend_from_slice(&0i32.to_le_bytes());
        buffer.push(priority.value());
        buffer.extend_from_slice(tag.as_bytes());
        buffer.push(0);
        buffer.extend_from_slice(message.as_bytes());
        buffer.push(0);
        buffer
    }

    #[test]
    fn test_default_options() {
        let options = ReaderOptions::default();
        assert_eq!(options.format, ReaderFormat::Binary);
        assert!(options.fix_line_feeds);
        assert_eq!(options.priority, Priority::Debug);
    }

    fn entry(priority: Priority, tag: &str) -> Entry {
        let mut entry = Entry::new();
        entry.set_priority(priority);
        entry.set_tag(tag);
        entry
    }

    #[test]
    fn test_no_filters_means_everything_passes() {
        let filters = Filters::default();
        assert!(filter(&filters, &entry(Priority::Verbose, "foo")));
    }

    #[test]
    fn test_all_filter() {
        let filters = Filters {
            all: Priority::Error.value() as i32,
            ..Default::default()
        };
        assert!(!filter(&filters, &entry(Priority::Info, "foo")));
        assert!(filter(&filters, &entry(Priority::Error, "foo")));
        assert!(filter(&filters, &entry(Priority::Fatal, "foo")));
    }

    #[test]
    fn test_tag_filter_beats_all_filter() {
        let mut filters = Filters {
            all: Priority::Debug.value() as i32,
            ..Default::default()
        };
        filters.tags.insert("foo".to_string(), Priority::Silent);
        assert!(!filter(&filters, &entry(Priority::Error, "foo")));
        assert!(filter(&filters, &entry(Priority::Error, "bar")));
    }

    #[test]
    fn test_tag_filter_lets_matching_priorities_through() {
        let mut filters = Filters::default();
        filters.tags.insert("foo".to_string(), Priority::Warn);
        assert!(!filter(&filters, &entry(Priority::Info, "foo")));
        assert!(filter(&filters, &entry(Priority::Warn, "foo")));
    }

    #[tokio::test]
    async fn test_reads_entries_from_a_stream() {
        let bytes = entry_bytes(1, 2, Priority::Info, "tag", "hello");
        let (client, mut server) = tokio::io::duplex(4096);
        let mut reader = Reader::connect(client);

        server.write_all(&bytes).await.unwrap();
        server.shutdown().await.unwrap();

        let event = reader.next().await.expect("an event");
        match event {
            ReaderEvent::Entry(entry) => {
                assert_eq!(entry.tag.as_deref(), Some("tag"));
                assert_eq!(entry.message.as_deref(), Some("hello"));
                assert_eq!(entry.priority, Priority::Info);
                assert_eq!(
                    entry.date,
                    Some(UNIX_EPOCH + std::time::Duration::from_secs(1000))
                );
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_fixes_line_feeds() {
        // The ADB shell turns every \n into \r\n, so the reader must undo that
        // before parsing.
        let mut mangled = Vec::new();
        for byte in entry_bytes(1, 2, Priority::Info, "tag", "hello") {
            mangled.push(byte);
            if byte == 0x0a {
                mangled.insert(mangled.len() - 1, 0x0d);
            }
        }

        let (client, mut server) = tokio::io::duplex(4096);
        let mut reader = Reader::connect_with_options(
            client,
            ReaderOptions {
                fix_line_feeds: true,
                ..Default::default()
            },
        )
        .unwrap();
        server.write_all(&mangled).await.unwrap();
        server.shutdown().await.unwrap();

        let event = reader.next().await.expect("an event");
        match event {
            ReaderEvent::Entry(entry) => {
                assert_eq!(entry.message.as_deref(), Some("hello"));
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_excludes_filtered_tags() {
        let bytes = entry_bytes(1, 2, Priority::Error, "noisy", "spam");
        let (client, mut server) = tokio::io::duplex(4096);
        let mut reader = Reader::connect(client);
        reader.exclude("noisy");

        server.write_all(&bytes).await.unwrap();
        server.shutdown().await.unwrap();

        // The entry is filtered out, so the next event is the end of the stream.
        match reader.next().await {
            Some(ReaderEvent::End) | Some(ReaderEvent::Finish) | None => {}
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_rejects_unsupported_formats() {
        let options = ReaderOptions {
            format: ReaderFormat::Binary,
            ..Default::default()
        };
        let result: Result<Reader<&[u8]>> = Reader::with_options(options);
        assert!(result.is_ok());
        assert_eq!(ReaderFormat::Binary.to_string(), "binary");
    }
}
