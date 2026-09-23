//! Tests for the `host:*` commands.
//!
//! Ported from `test/adb/command/host/*.ts`.

mod common;

use adbkit::command::host;

#[tokio::test]
async fn test_version_sends_host_version() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"0x1234").await;
    mock.end().await;

    let version = host::version(&mut conn).await.unwrap();

    common::assert_written(&mut mock, "host:version").await;
    assert_eq!(version, 0x1234);
}

#[tokio::test]
async fn test_version_hex_string() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"1234").await;
    mock.end().await;

    assert_eq!(host::version(&mut conn).await.unwrap(), 0x1234);
}

#[tokio::test]
async fn test_version_old_style() {
    let (mut mock, mut conn) = common::mock();
    mock.feed(b"1234").await;
    mock.end().await;

    assert_eq!(host::version(&mut conn).await.unwrap(), 0x1234);
}

#[tokio::test]
async fn test_version_fail() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_fail("because").await;
    mock.end().await;

    let err = host::version(&mut conn).await.unwrap_err();
    assert_eq!(err.to_string(), "Failure: 'because'");
}

#[tokio::test]
async fn test_version_unexpected() {
    // Anything that is neither OKAY nor FAIL nor a hexadecimal number is
    // rejected.
    let (mut mock, mut conn) = common::mock();
    mock.feed(b"ZZZZ").await;
    mock.end().await;

    let err = host::version(&mut conn).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected 'ZZZZ', was expecting a hexadecimal version number"
    );
}

#[tokio::test]
async fn test_connect() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"connected to 192.168.2.2:5555").await;
    mock.end().await;

    let id = host::connect(&mut conn, "192.168.2.2", 5555).await.unwrap();

    common::assert_written(&mut mock, "host:connect:192.168.2.2:5555").await;
    assert_eq!(id, "192.168.2.2:5555");
}

#[tokio::test]
async fn test_connect_already_connected() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"already connected to 192.168.2.2:5555")
        .await;
    mock.end().await;

    assert_eq!(
        host::connect(&mut conn, "192.168.2.2", 5555).await.unwrap(),
        "192.168.2.2:5555"
    );
}

#[tokio::test]
async fn test_connect_unable() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"unable to connect to 192.168.2.2:5555")
        .await;
    mock.end().await;

    let err = host::connect(&mut conn, "192.168.2.2", 5555)
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "unable to connect to 192.168.2.2:5555");
}

#[tokio::test]
async fn test_disconnect() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"").await;
    mock.end().await;

    let id = host::disconnect(&mut conn, "192.168.2.2", 5555)
        .await
        .unwrap();

    common::assert_written(&mut mock, "host:disconnect:192.168.2.2:5555").await;
    assert_eq!(id, "192.168.2.2:5555");
}

#[tokio::test]
async fn test_list_devices() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"a\tdevice\nb\toffline\n").await;
    mock.end().await;

    let devices = host::devices(&mut conn).await.unwrap();

    common::assert_written(&mut mock, "host:devices").await;
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].id, "a");
    assert_eq!(devices[0].r#type, adbkit::DeviceType::Device);
    assert_eq!(devices[1].id, "b");
    assert_eq!(devices[1].r#type, adbkit::DeviceType::Offline);
}

#[tokio::test]
async fn test_list_devices_with_paths() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"1234 device usb:1-1 product model device transport\n")
        .await;
    mock.end().await;

    let devices = host::devices_with_paths(&mut conn).await.unwrap();

    common::assert_written(&mut mock, "host:devices-l").await;
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, "1234");
    assert_eq!(devices[0].path, "usb:1-1");
    assert_eq!(devices[0].product, "product");
    assert_eq!(devices[0].model, "model");
    assert_eq!(devices[0].device, "device");
    assert_eq!(devices[0].transport_id, "transport");
}

#[tokio::test]
async fn test_kill() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    assert!(host::kill(&mut conn).await.unwrap());
    common::assert_written(&mut mock, "host:kill").await;
}

#[tokio::test]
async fn test_transport() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    assert!(host::transport(&mut conn, "serial").await.unwrap());
    common::assert_written(&mut mock, "host:transport:serial").await;
}

#[tokio::test]
async fn test_devices_fail() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_fail("nope").await;
    mock.end().await;

    let err = host::devices(&mut conn).await.unwrap_err();
    assert_eq!(err.to_string(), "Failure: 'nope'");
}
