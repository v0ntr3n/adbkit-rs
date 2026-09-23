//! Tests for the sync service.
//!
//! Ported from `test/adb/sync.ts`. The tests that need a real device are marked
//! `#[ignore]` and can be run with `cargo test -- --ignored` while a device is
//! connected, just like the `RUN_DEVICE_TESTS` tests of the original suite.

mod common;

use adbkit::protocol::Protocol;
use adbkit::sync::{Entry, Stats, Sync};

/// Feeds a `STAT` reply.
async fn feed_stat(mock: &mut common::Mock, mode: u32, size: u32, mtime: u32) {
    let mut stat = Vec::new();
    stat.extend_from_slice(&mode.to_le_bytes());
    stat.extend_from_slice(&size.to_le_bytes());
    stat.extend_from_slice(&mtime.to_le_bytes());
    mock.feed(Protocol::STAT.as_bytes()).await;
    mock.feed(&stat).await;
    mock.end().await;
}

#[tokio::test]
async fn test_stat() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);
    feed_stat(&mut mock, Stats::S_IFREG | 0o644, 1234, 1000).await;

    let stats = sync.stat("/system/build.prop").await.unwrap();

    // Sync commands carry a raw little-endian length, unlike the host protocol.
    let written = mock.written().await;
    assert_eq!(&written[..4], b"STAT");
    assert_eq!(&written[4..8], &(18u32).to_le_bytes());
    assert_eq!(&written[8..], b"/system/build.prop");
    assert_eq!(stats.mode, Stats::S_IFREG | 0o644);
    assert_eq!(stats.size, 1234);
    assert_eq!(stats.mtime, 1000);
    assert!(stats.is_file());
    assert!(!stats.is_directory());
}

#[tokio::test]
async fn test_stat_no_such_file() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);
    feed_stat(&mut mock, 0, 0, 0).await;

    let err = sync.stat("/non-existing").await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "ENOENT, no such file or directory '/non-existing'"
    );
}

#[tokio::test]
async fn test_stat_fail() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);
    mock.feed_fail("nope").await;
    mock.end().await;

    let err = sync.stat("/").await.unwrap_err();
    assert_eq!(err.to_string(), "Failure: 'nope'");
}

#[tokio::test]
async fn test_sta2() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);

    let mut stat = vec![0u8; 68];
    stat[20..24].copy_from_slice(&(Stats::S_IFREG).to_le_bytes());
    stat[36..44].copy_from_slice(&1234u64.to_le_bytes());
    stat[52..60].copy_from_slice(&1000u64.to_le_bytes());
    mock.feed(Protocol::STA2.as_bytes()).await;
    mock.feed(&stat).await;
    mock.end().await;

    let stats = sync.sta2("/system/build.prop").await.unwrap();
    assert_eq!(stats.mode, Stats::S_IFREG);
    assert_eq!(stats.size, 1234);
    assert_eq!(stats.size_big, 1234);
    assert_eq!(stats.mtime, 1000);
}

#[tokio::test]
async fn test_readdir() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);

    let dent = |name: &str| {
        let mut data = Vec::new();
        data.extend_from_slice(&(Stats::S_IFREG).to_le_bytes());
        data.extend_from_slice(&100u32.to_le_bytes());
        data.extend_from_slice(&1000u32.to_le_bytes());
        data.extend_from_slice(&(name.len() as u32).to_le_bytes());
        data.extend_from_slice(name.as_bytes());
        data
    };
    mock.feed(Protocol::DENT.as_bytes()).await;
    mock.feed(&dent("build.prop")).await;
    mock.feed(Protocol::DENT.as_bytes()).await;
    mock.feed(&dent("system")).await;
    mock.feed(Protocol::DONE.as_bytes()).await;
    mock.feed(&[0u8; 16]).await;
    mock.end().await;

    let files = sync.readdir("/system").await.unwrap();

    let written = mock.written().await;
    assert_eq!(&written[..4], b"LIST");
    assert_eq!(&written[4..8], &(7u32).to_le_bytes());
    assert_eq!(&written[8..], b"/system");
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].name, "build.prop");
    assert!(files[0].is_file());
    assert_eq!(files[1].name, "system");
}

#[tokio::test]
async fn test_readdir_skips_dot_entries() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);

    let dent = |name: &str| {
        let mut data = Vec::new();
        data.extend_from_slice(&(Stats::S_IFDIR).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(name.len() as u32).to_le_bytes());
        data.extend_from_slice(name.as_bytes());
        data
    };
    mock.feed(Protocol::DENT.as_bytes()).await;
    mock.feed(&dent(".")).await;
    mock.feed(Protocol::DENT.as_bytes()).await;
    mock.feed(&dent("..")).await;
    mock.feed(Protocol::DENT.as_bytes()).await;
    mock.feed(&dent("bin")).await;
    mock.feed(Protocol::DONE.as_bytes()).await;
    mock.feed(&[0u8; 16]).await;
    mock.end().await;

    let files: Vec<Entry> = sync.readdir("/").await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "bin");
    assert!(files[0].is_directory());
}

#[tokio::test]
async fn test_pull() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);

    mock.feed(Protocol::DATA.as_bytes()).await;
    mock.feed(&10u32.to_le_bytes()).await;
    mock.feed(b"build.prop").await;
    mock.feed(Protocol::DONE.as_bytes()).await;
    mock.feed(&0u32.to_le_bytes()).await;
    mock.end().await;

    let transfer = sync
        .pull("/system/build.prop", Default::default())
        .await
        .unwrap();
    let pulled = common::read_all(transfer).await.unwrap();

    assert_eq!(pulled, b"build.prop".to_vec());
}

#[tokio::test]
async fn test_pull_reports_progress() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);

    mock.feed(Protocol::DATA.as_bytes()).await;
    mock.feed(&5u32.to_le_bytes()).await;
    mock.feed(b"hello").await;
    mock.feed(Protocol::DATA.as_bytes()).await;
    mock.feed(&6u32.to_le_bytes()).await;
    mock.feed(b" world").await;
    mock.feed(Protocol::DONE.as_bytes()).await;
    mock.feed(&0u32.to_le_bytes()).await;
    mock.end().await;

    let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let progress_clone = progress.clone();
    let options = adbkit::sync::PullOptions {
        progress: Some(Box::new(move |bytes| {
            progress_clone.store(bytes, std::sync::atomic::Ordering::SeqCst);
        })),
    };

    let transfer = sync.pull("/foo", options).await.unwrap();
    let pulled = common::read_all(transfer).await.unwrap();

    assert_eq!(pulled, b"hello world".to_vec());
    assert_eq!(progress.load(std::sync::atomic::Ordering::SeqCst), 11);
}

#[tokio::test]
async fn test_pull_fail() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);

    mock.feed_fail("no such file").await;
    mock.end().await;

    let transfer = sync.pull("/nope", Default::default()).await.unwrap();
    // The failure surfaces as an I/O error carrying the failure message.
    let err = common::read_all(transfer).await.unwrap_err();
    assert!(
        err.to_string().contains("Failure: 'no such file'"),
        "{}",
        err
    );
}

#[tokio::test]
async fn test_push() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);
    // The device only acknowledges once everything has been sent.
    mock.feed_okay().await;
    mock.feed(&0u32.to_le_bytes()).await;
    mock.end().await;

    let data = b"hello world".to_vec();
    let pushed = sync
        .push(&data[..], "/data/local/tmp/foo", None, Default::default())
        .await
        .unwrap();

    assert_eq!(pushed, 11);
    let written = mock.written().await;
    // SEND <path>,<mode>
    let mode = adbkit::sync::DEFAULT_CHMOD | adbkit::sync::Stats::S_IFREG;
    let arg = format!("/data/local/tmp/foo,{}", mode);
    assert_eq!(&written[..4], b"SEND");
    assert_eq!(&written[4..8], &(arg.len() as u32).to_le_bytes());
    assert_eq!(&written[8..8 + arg.len()], arg.as_bytes());
    // DATA <length>
    let data_offset = 8 + arg.len();
    assert_eq!(&written[data_offset..data_offset + 4], b"DATA");
    assert_eq!(
        &written[data_offset + 4..data_offset + 8],
        &11u32.to_le_bytes()
    );
    assert_eq!(&written[data_offset + 8..data_offset + 19], b"hello world");
    // DONE <timestamp>
    let done_offset = data_offset + 19;
    assert_eq!(&written[done_offset..done_offset + 4], b"DONE");
    assert_eq!(written.len(), done_offset + 8);
}

#[tokio::test]
async fn test_push_reports_progress() {
    let (mut mock, conn) = common::mock();
    let mut sync = Sync::new(conn);
    mock.feed_okay().await;
    mock.feed(&0u32.to_le_bytes()).await;
    mock.end().await;

    let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let progress_clone = progress.clone();
    let options = adbkit::sync::PushOptions {
        progress: Some(Box::new(move |bytes| {
            progress_clone.store(bytes, std::sync::atomic::Ordering::SeqCst);
        })),
    };

    let data = b"0123456789".to_vec();
    sync.push(&data[..], "/data/local/tmp/foo", None, options)
        .await
        .unwrap();

    assert_eq!(progress.load(std::sync::atomic::Ordering::SeqCst), 10);
}

/// Tests that need a real device, run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore = "requires a connected Android device"]
async fn test_device_stat() {
    use adbkit::Adb;

    let client = Adb::create_client(Default::default());
    for device in client.list_devices().await.unwrap() {
        let stats = client
            .get_device(&device.id)
            .stat("/system/build.prop")
            .await;
        assert!(stats.is_ok(), "{}", stats.unwrap_err());
    }
}

#[tokio::test]
#[ignore = "requires a connected Android device"]
async fn test_device_readdir() {
    use adbkit::Adb;

    let client = Adb::create_client(Default::default());
    for device in client.list_devices().await.unwrap() {
        let files = client.get_device(&device.id).readdir("/").await;
        assert!(files.is_ok(), "{}", files.unwrap_err());
    }
}

#[tokio::test]
#[ignore = "requires a connected Android device"]
async fn test_device_pull() {
    use adbkit::Adb;

    let client = Adb::create_client(Default::default());
    for device in client.list_devices().await.unwrap() {
        let transfer = client
            .get_device(&device.id)
            .pull("/system/build.prop", Default::default())
            .await;
        assert!(transfer.is_ok(), "{}", transfer.unwrap_err());
        let data = common::read_all(transfer.unwrap()).await.unwrap();
        assert!(!data.is_empty());
    }
}

#[tokio::test]
#[ignore = "requires a connected Android device"]
async fn test_device_push() {
    use adbkit::Adb;

    let client = Adb::create_client(Default::default());
    let path = "/data/local/tmp/_rust.test";
    for device in client.list_devices().await.unwrap() {
        let device = client.get_device(&device.id);
        let data = b"hello from rust".to_vec();
        let pushed = device
            .push(
                adbkit::PushSource::from(std::io::Cursor::new(data)),
                path,
                None,
                Default::default(),
            )
            .await;
        assert!(pushed.is_ok(), "{}", pushed.unwrap_err());
        let removed = device
            .shell_args(&[
                adbkit::command::Arg::from("rm"),
                adbkit::command::Arg::from("-f"),
                adbkit::command::Arg::from(path),
            ])
            .await;
        assert!(removed.is_ok());
    }
}
