//! The sync service: pushing, pulling and listing files.
//!
//! Ported from `src/adb/sync.ts`, `src/adb/sync/stats.ts`,
//! `src/adb/sync/entry.ts`, `src/adb/sync/pulltransfer.ts` and
//! `src/adb/sync/pushtransfer.ts`.
//!
//! The TypeScript implementation returns `EventEmitter`s (`PushTransfer` and
//! `PullTransfer`) that stream data in the background. Here, pushes are awaited
//! directly and pulls are [`AsyncRead`] streams, with progress reported through
//! an optional callback.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncRead, ReadBuf};

use crate::connection::{AdbStream, Connection};
use crate::error::{Error, Result};
use crate::protocol::{parse_reply, Protocol, Reply};

/// The path used for temporary files on the device.
pub const TEMP_PATH: &str = "/data/local/tmp";
/// The default mode for pushed files (`0o644`).
pub const DEFAULT_CHMOD: u32 = 0o644;
/// The maximum amount of data sent in a single sync packet.
pub const DATA_MAX_LENGTH: usize = 65536;

/// File statistics, as returned by [`Sync::stat`].
///
/// Ported from `src/adb/sync/stats.ts`, which extends Node's `fs.Stats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// The raw mode.
    pub mode: u32,
    /// The file size.
    pub size: u64,
    /// The raw size, as a 64-bit integer.
    pub size_big: u64,
    /// The time of last modification, in seconds since the epoch.
    pub mtime: u64,
    /// The time of last modification, in milliseconds since the epoch.
    pub mtime_ms: u64,
}

impl Stats {
    /// Creates new statistics.
    pub fn new(mode: u32, size: u64, mtime: u64) -> Self {
        Stats {
            mode,
            size,
            size_big: size,
            mtime,
            mtime_ms: mtime * 1000,
        }
    }

    /// Bit mask for the file type bit fields.
    pub const S_IFMT: u32 = 0o170000;
    /// Socket.
    pub const S_IFSOCK: u32 = 0o140000;
    /// Symbolic link.
    pub const S_IFLNK: u32 = 0o120000;
    /// Regular file.
    pub const S_IFREG: u32 = 0o100000;
    /// Block device.
    pub const S_IFBLK: u32 = 0o060000;
    /// Directory.
    pub const S_IFDIR: u32 = 0o040000;
    /// Character device.
    pub const S_IFCHR: u32 = 0o020000;
    /// FIFO.
    pub const S_IFIFO: u32 = 0o010000;

    /// Whether this is a socket.
    pub fn is_socket(&self) -> bool {
        self.mode & Self::S_IFMT == Self::S_IFSOCK
    }

    /// Whether this is a symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.mode & Self::S_IFMT == Self::S_IFLNK
    }

    /// Whether this is a regular file.
    pub fn is_file(&self) -> bool {
        self.mode & Self::S_IFMT == Self::S_IFREG
    }

    /// Whether this is a block device.
    pub fn is_block_device(&self) -> bool {
        self.mode & Self::S_IFMT == Self::S_IFBLK
    }

    /// Whether this is a directory.
    pub fn is_directory(&self) -> bool {
        self.mode & Self::S_IFMT == Self::S_IFDIR
    }

    /// Whether this is a character device.
    pub fn is_character_device(&self) -> bool {
        self.mode & Self::S_IFMT == Self::S_IFCHR
    }

    /// Whether this is a FIFO.
    pub fn is_fifo(&self) -> bool {
        self.mode & Self::S_IFMT == Self::S_IFIFO
    }

    /// The time of last modification.
    pub fn modified(&self) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_millis(self.mtime_ms)
    }
}

/// A directory entry, as returned by [`Sync::readdir`].
///
/// Ported from `src/adb/sync/entry.ts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The file name.
    pub name: String,
    /// The entry statistics.
    pub stats: Stats,
}

impl Entry {
    /// Whether this entry is a regular file.
    pub fn is_file(&self) -> bool {
        self.stats.is_file()
    }

    /// Whether this entry is a directory.
    pub fn is_directory(&self) -> bool {
        self.stats.is_directory()
    }

    /// Whether this entry is a symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.stats.is_symlink()
    }

    /// Whether this entry is a socket.
    pub fn is_socket(&self) -> bool {
        self.stats.is_socket()
    }

    /// Whether this entry is a block device.
    pub fn is_block_device(&self) -> bool {
        self.stats.is_block_device()
    }

    /// Whether this entry is a character device.
    pub fn is_character_device(&self) -> bool {
        self.stats.is_character_device()
    }

    /// Whether this entry is a FIFO.
    pub fn is_fifo(&self) -> bool {
        self.stats.is_fifo()
    }
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// A progress callback, called with the number of bytes transferred so far.
pub type ProgressCallback = Box<dyn FnMut(u64) + Send>;

/// Options for [`Sync::push`].
#[derive(Default)]
pub struct PushOptions {
    /// Called whenever a chunk has been transferred.
    pub progress: Option<ProgressCallback>,
}

/// Options for [`Sync::pull`].
#[derive(Default)]
pub struct PullOptions {
    /// Called whenever a chunk has been transferred.
    pub progress: Option<ProgressCallback>,
}

/// A file transfer: reads a file from the device.
///
/// Returned by [`Sync::pull`]. Read from it as you would from any other
/// [`AsyncRead`]; it is done when reading returns `Ok(0)`.
pub struct PullTransfer<'a, S>
where
    S: AdbStream,
{
    owner: Owner<'a, S>,
    state: PullState,
    bytes_transferred: u64,
    progress: Option<ProgressCallback>,
    done: bool,
}

enum Owner<'a, S>
where
    S: AdbStream,
{
    /// The transfer borrows a sync connection, which can be reused afterwards.
    Borrowed(&'a mut Sync<S>),
    /// The transfer owns the sync connection, which is closed when it is
    /// dropped.
    Owned(Sync<S>),
}

enum PullState {
    /// Waiting for a `DATA`, `DONE` or `FAIL` code.
    Reply,
    /// Waiting for the length of a `DATA` payload.
    Length,
    /// Streaming `remaining` bytes of payload.
    Data { remaining: u64 },
    /// Discarding `remaining` bytes (the id that follows `DONE`).
    Discard { remaining: usize },
    /// The transfer completed.
    Done,
    /// The transfer failed with this error.
    Failed(Error),
    /// A `FAIL` was received; its message still has to be read.
    FailedFailure,
    /// Reading the length of a `FAIL` message.
    FailureHeader,
    /// Reading the `FAIL` message.
    FailureData {
        /// The number of bytes still to read.
        remaining: usize,
        /// The bytes read so far.
        collected: Vec<u8>,
    },
    /// A placeholder used while the state machine is being driven.
    Busy,
}

impl<'a, S> PullTransfer<'a, S>
where
    S: AdbStream,
{
    /// The number of bytes transferred so far.
    pub fn bytes_transferred(&self) -> u64 {
        self.bytes_transferred
    }

    /// Reads the remaining data into a vector.
    pub async fn read_to_vec(mut self) -> Result<Vec<u8>> {
        use tokio::io::AsyncReadExt;

        let mut out = Vec::new();
        self.read_to_end(&mut out).await?;
        Ok(out)
    }

    fn poll_fill(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        loop {
            // Take ownership of the state so that we can assign to `self.state`
            // while borrowing the parser.
            let state = std::mem::replace(&mut self.state, PullState::Busy);
            match state {
                PullState::Busy => unreachable!("the state machine never rests on Busy"),
                PullState::Reply => {
                    let mut code = [0u8; 4];
                    let mut read = ReadBuf::new(&mut code);
                    match self.poll_parser(cx, &mut read) {
                        Poll::Ready(Ok(())) => {
                            let code = String::from_utf8_lossy(&code).into_owned();
                            self.state = match parse_reply(&code) {
                                Some(Reply::Data) => PullState::Length,
                                Some(Reply::Done) => {
                                    // A `DONE` is followed by a 4 byte id.
                                    PullState::Discard { remaining: 4 }
                                }
                                Some(Reply::Fail) => PullState::FailedFailure,
                                _ => {
                                    PullState::Failed(Error::unexpected(code, "DATA, DONE or FAIL"))
                                }
                            };
                        }
                        Poll::Ready(Err(err)) => {
                            self.state = PullState::Reply;
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            self.state = PullState::Reply;
                            return Poll::Pending;
                        }
                    }
                }
                PullState::Length => {
                    let mut length = [0u8; 4];
                    let mut read = ReadBuf::new(&mut length);
                    match self.poll_parser(cx, &mut read) {
                        Poll::Ready(Ok(())) => {
                            let length = u32::from_le_bytes(length);
                            self.state = PullState::Data {
                                remaining: length as u64,
                            };
                        }
                        Poll::Ready(Err(err)) => {
                            self.state = PullState::Length;
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            self.state = PullState::Length;
                            return Poll::Pending;
                        }
                    }
                }
                PullState::Data { remaining } => {
                    if remaining == 0 {
                        self.state = PullState::Reply;
                        continue;
                    }
                    if buf.remaining() == 0 {
                        self.state = PullState::Data { remaining };
                        return Poll::Ready(Ok(()));
                    }
                    let want = remaining.min(buf.remaining() as u64) as usize;
                    let mut chunk = vec![0u8; want];
                    let mut read = ReadBuf::new(&mut chunk);
                    match self.poll_parser(cx, &mut read) {
                        Poll::Ready(Ok(())) => {
                            let filled = read.filled().to_vec();
                            buf.put_slice(&filled);
                            let transferred = filled.len() as u64;
                            self.bytes_transferred += transferred;
                            self.state = PullState::Data {
                                remaining: remaining - transferred,
                            };
                            if let Some(progress) = self.progress.as_mut() {
                                progress(self.bytes_transferred);
                            }
                            return Poll::Ready(Ok(()));
                        }
                        Poll::Ready(Err(err)) => {
                            self.state = PullState::Data { remaining };
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            self.state = PullState::Data { remaining };
                            return Poll::Pending;
                        }
                    }
                }
                PullState::Discard { remaining } => {
                    if remaining == 0 {
                        self.state = PullState::Done;
                        continue;
                    }
                    let mut chunk = vec![0u8; remaining];
                    let mut read = ReadBuf::new(&mut chunk);
                    match self.poll_parser(cx, &mut read) {
                        Poll::Ready(Ok(())) => {
                            let filled = read.filled().len();
                            self.state = PullState::Discard {
                                remaining: remaining - filled,
                            };
                        }
                        Poll::Ready(Err(err)) => {
                            self.state = PullState::Discard { remaining };
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            self.state = PullState::Discard { remaining };
                            return Poll::Pending;
                        }
                    }
                }
                PullState::Done => {
                    self.done = true;
                    return Poll::Ready(Ok(()));
                }
                PullState::Failed(err) => {
                    self.state = PullState::Done;
                    self.done = true;
                    return Poll::Ready(Err(std::io::Error::other(err)));
                }
                PullState::FailedFailure => {
                    // A `FAIL` is followed by a length-prefixed message.
                    self.state = PullState::FailureHeader;
                }
                PullState::FailureHeader => {
                    let mut length = [0u8; 4];
                    let mut read = ReadBuf::new(&mut length);
                    match self.poll_parser(cx, &mut read) {
                        Poll::Ready(Ok(())) => {
                            let code = String::from_utf8_lossy(&length).into_owned();
                            match usize::from_str_radix(&code, 16) {
                                Ok(length) => {
                                    self.state = PullState::FailureData {
                                        remaining: length,
                                        collected: Vec::with_capacity(length),
                                    };
                                }
                                Err(_) => {
                                    self.state = PullState::Failed(Error::unexpected(
                                        code,
                                        "a hexadecimal length",
                                    ));
                                }
                            }
                        }
                        Poll::Ready(Err(err)) => {
                            self.state = PullState::FailureHeader;
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            self.state = PullState::FailureHeader;
                            return Poll::Pending;
                        }
                    }
                }
                PullState::FailureData {
                    mut remaining,
                    mut collected,
                } => {
                    if remaining == 0 {
                        self.state = PullState::Failed(Error::Failure(
                            String::from_utf8_lossy(&collected).into_owned(),
                        ));
                        continue;
                    }
                    let mut chunk = vec![0u8; remaining];
                    let mut read = ReadBuf::new(&mut chunk);
                    match self.poll_parser(cx, &mut read) {
                        Poll::Ready(Ok(())) => {
                            let filled = read.filled();
                            collected.extend_from_slice(filled);
                            remaining -= filled.len();
                            self.state = PullState::FailureData {
                                remaining,
                                collected,
                            };
                        }
                        Poll::Ready(Err(err)) => {
                            self.state = PullState::FailureData {
                                remaining,
                                collected,
                            };
                            return Poll::Ready(Err(err));
                        }
                        Poll::Pending => {
                            self.state = PullState::FailureData {
                                remaining,
                                collected,
                            };
                            return Poll::Pending;
                        }
                    }
                }
            }
        }
    }

    /// Reads from the parser behind the transfer's owner.
    fn poll_parser(
        &mut self,
        cx: &mut Context<'_>,
        read: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let parser = match &mut self.owner {
            Owner::Borrowed(sync) => sync.connection.parser(),
            Owner::Owned(sync) => sync.connection.parser(),
        };
        std::pin::Pin::new(parser).poll_read(cx, read)
    }
}

impl<'a, S> AsyncRead for PullTransfer<'a, S>
where
    S: AdbStream,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.done {
            return Poll::Ready(Ok(()));
        }
        self.poll_fill(cx, buf)
    }
}

impl<'a, S> std::fmt::Debug for PullTransfer<'a, S>
where
    S: AdbStream,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PullTransfer")
            .field("bytes_transferred", &self.bytes_transferred)
            .finish_non_exhaustive()
    }
}

/// A sync service connection.
///
/// Ported from `src/adb/sync.ts`.
pub struct Sync<S>
where
    S: AdbStream,
{
    pub(crate) connection: Connection<S>,
}

impl<S> Sync<S>
where
    S: AdbStream,
{
    /// Wraps a connection that has already entered sync mode.
    pub fn new(connection: Connection<S>) -> Self {
        Sync { connection }
    }

    /// The temporary path used for a file on the device.
    pub fn temp_file(&self, path: &str) -> String {
        temp(path)
    }

    /// Retrieves information about the given path.
    ///
    /// Ported from `Sync.stat()`.
    pub async fn stat(&mut self, path: &str) -> Result<Stats> {
        self.send_command_with_arg(Protocol::STAT, path).await?;
        match self.read_reply().await? {
            Reply::Stat => {
                let stat = self.connection.parser().read_bytes(12).await?;
                let mode = read_u32(&stat, 0);
                let size = read_u32(&stat, 4);
                let mtime = read_u32(&stat, 8);
                if mode == 0 {
                    Err(enoent(path))
                } else {
                    Ok(Stats::new(mode, size as u64, mtime as u64))
                }
            }
            reply => self
                .connection
                .parser()
                .unexpected(reply.to_string(), "STAT or FAIL"),
        }
    }

    /// Retrieves information about the given path, including 64-bit sizes.
    ///
    /// Ported from `Sync.sta2()`.
    pub async fn sta2(&mut self, path: &str) -> Result<Stats> {
        self.send_command_with_arg(Protocol::STA2, path).await?;
        match self.read_reply().await? {
            Reply::Sta2 => {
                let stat = self.connection.parser().read_bytes(68).await?;
                let mode = read_u32(&stat, 20);
                let size = read_u64(&stat, 36);
                let mtime = read_u64(&stat, 52);
                if mode == 0 {
                    Err(enoent(path))
                } else {
                    Ok(Stats::new(mode, size, mtime))
                }
            }
            reply => self
                .connection
                .parser()
                .unexpected(reply.to_string(), "STA2 or FAIL"),
        }
    }

    /// Lists the contents of a directory.
    ///
    /// Ported from `Sync.readdir()`.
    pub async fn readdir(&mut self, path: &str) -> Result<Vec<Entry>> {
        let mut files: Vec<Entry> = Vec::new();
        self.send_command_with_arg(Protocol::LIST, path).await?;
        loop {
            match self.read_reply().await? {
                Reply::Dent => {
                    let stat = self.connection.parser().read_bytes(16).await?;
                    let mode = read_u32(&stat, 0);
                    let size = read_u32(&stat, 4);
                    let mtime = read_u32(&stat, 8);
                    let namelen = read_u32(&stat, 12);
                    let name = self
                        .connection
                        .parser()
                        .read_bytes(namelen as usize)
                        .await?;
                    let name = String::from_utf8_lossy(&name).into_owned();
                    // Skip '.' and '..' to match Node's fs.readdir().
                    if name != "." && name != ".." {
                        files.push(Entry {
                            name,
                            stats: Stats::new(mode, size as u64, mtime as u64),
                        });
                    }
                }
                Reply::Done => {
                    let _ = self.connection.parser().read_bytes(16).await?;
                    return Ok(files);
                }
                reply => {
                    return self
                        .connection
                        .parser()
                        .unexpected(reply.to_string(), "DENT, DONE or FAIL")
                }
            }
        }
    }

    /// Pushes a file to the device.
    ///
    /// `contents` is anything that can be read asynchronously; use
    /// [`Sync::push_file`] to push a file from the local file system.
    ///
    /// Ported from `Sync.push()`.
    pub async fn push<R>(
        &mut self,
        contents: R,
        path: &str,
        mode: Option<u32>,
        options: PushOptions,
    ) -> Result<u64>
    where
        R: AsyncRead + Unpin,
    {
        let mode = mode.unwrap_or(DEFAULT_CHMOD) | Stats::S_IFREG;
        let time_stamp = now_seconds();
        self.send_command_with_arg(Protocol::SEND, &format!("{},{}", path, mode))
            .await?;

        let mut contents = contents;
        let mut total = 0u64;
        let mut progress = options.progress;
        loop {
            let mut chunk = vec![0u8; DATA_MAX_LENGTH];
            let filled = tokio::io::AsyncReadExt::read(&mut contents, &mut chunk).await?;
            if filled == 0 {
                break;
            }
            let data = &chunk[..filled];
            self.send_command_with_length(Protocol::DATA, data.len() as u32)
                .await?;
            self.connection.write(data).await?;
            total += filled as u64;
            if let Some(progress) = progress.as_mut() {
                progress(total);
            }
        }
        self.send_command_with_length(Protocol::DONE, time_stamp as u32)
            .await?;

        // Wait for the final OKAY (or FAIL).
        match self.read_reply().await? {
            Reply::Okay => {
                let _ = self.connection.parser().read_bytes(4).await?;
                Ok(total)
            }
            reply => self
                .connection
                .parser()
                .unexpected(reply.to_string(), "OKAY or FAIL"),
        }
    }

    /// Pushes a file from the local file system.
    ///
    /// Ported from `Sync.pushFile()`.
    pub async fn push_file(
        &mut self,
        file: &str,
        path: &str,
        mode: Option<u32>,
        options: PushOptions,
    ) -> Result<u64> {
        let file = tokio::fs::File::open(file).await?;
        self.push(file, path, mode, options).await
    }

    /// Pulls a file from the device.
    ///
    /// The returned transfer borrows this connection, which can be used again
    /// once the transfer is done.
    ///
    /// Ported from `Sync.pull()`.
    pub async fn pull(&mut self, path: &str, options: PullOptions) -> Result<PullTransfer<'_, S>> {
        self.send_command_with_arg(Protocol::RECV, path).await?;
        Ok(PullTransfer {
            owner: Owner::Borrowed(self),
            state: PullState::Reply,
            bytes_transferred: 0,
            progress: options.progress,
            done: false,
        })
    }

    /// Pulls a file from the device, consuming the connection.
    ///
    /// Ported from `Sync.pull()`, used by the one-shot
    /// `DeviceClient::pull()` helper.
    pub async fn pull_owned(
        self,
        path: &str,
        options: PullOptions,
    ) -> Result<PullTransfer<'static, S>> {
        let mut sync = self;
        sync.send_command_with_arg(Protocol::RECV, path).await?;
        Ok(PullTransfer {
            owner: Owner::Owned(sync),
            state: PullState::Reply,
            bytes_transferred: 0,
            progress: options.progress,
            done: false,
        })
    }

    /// Ends the sync connection.
    ///
    /// Ported from `Sync.end()`.
    pub async fn end(&mut self) {
        self.connection.end().await;
    }

    async fn read_reply(&mut self) -> Result<Reply> {
        crate::parser::read_reply(self.connection.parser()).await
    }

    async fn send_command_with_arg(&mut self, cmd: &str, arg: &str) -> Result<()> {
        crate::debug!("{} {}", cmd, arg);
        let arglen = arg.len() as u32;
        let mut payload = Vec::with_capacity(cmd.len() + 4 + arg.len());
        payload.extend_from_slice(cmd.as_bytes());
        payload.extend_from_slice(&arglen.to_le_bytes());
        payload.extend_from_slice(arg.as_bytes());
        self.connection.write(&payload).await
    }

    async fn send_command_with_length(&mut self, cmd: &str, length: u32) -> Result<()> {
        if cmd != Protocol::DATA {
            crate::debug!("{}", cmd);
        }
        let mut payload = Vec::with_capacity(cmd.len() + 4);
        payload.extend_from_slice(cmd.as_bytes());
        payload.extend_from_slice(&length.to_le_bytes());
        self.connection.write(&payload).await
    }
}

impl<S> std::fmt::Debug for Sync<S>
where
    S: AdbStream,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sync").finish_non_exhaustive()
    }
}

/// The temporary path used for a file on the device.
///
/// Ported from `Sync.temp()`.
pub fn temp(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    format!("{}/{}", TEMP_PATH, name)
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
        data.get(offset + 2).copied().unwrap_or(0),
        data.get(offset + 3).copied().unwrap_or(0),
    ])
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = data.get(offset + index).copied().unwrap_or(0);
    }
    u64::from_le_bytes(bytes)
}

fn enoent(path: &str) -> Error {
    Error::Device(format!("ENOENT, no such file or directory '{}'", path))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp() {
        assert_eq!(temp("/foo/bar.apk"), "/data/local/tmp/bar.apk");
        assert_eq!(temp("bar.apk"), "/data/local/tmp/bar.apk");
        assert_eq!(temp(""), "/data/local/tmp/");
    }

    #[test]
    fn test_stats() {
        let stats = Stats::new(Stats::S_IFREG | 0o644, 1234, 1000);
        assert!(stats.is_file());
        assert!(!stats.is_directory());
        assert_eq!(stats.size, 1234);
        assert_eq!(stats.size_big, 1234);
        assert_eq!(stats.mtime, 1000);
        assert_eq!(stats.mtime_ms, 1_000_000);
    }

    #[test]
    fn test_entry() {
        let entry = Entry {
            name: "build.prop".to_string(),
            stats: Stats::new(Stats::S_IFDIR, 0, 0),
        };
        assert_eq!(entry.to_string(), "build.prop");
        assert!(entry.is_directory());
        assert!(!entry.is_file());
    }
}
