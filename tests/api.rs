//! Tests for the public API.
//!
//! Ported from `test/adb.ts` and `test/adb/util.ts`, which check that the
//! package exports everything it promises.

use adbkit::{
    Adb, Auth, CpuStats, Device, DeviceClient, DeviceType, DeviceWithPath, ExtendedPublicKey,
    Features, Forward, FramebufferMeta, FramebufferStream, JdwpTracker, Loads, Parser, ProcStat,
    Properties, Protocol, PullTransfer, PushSource, Reverse, ScreenshotStream, ShellStream, Stats,
    Sync, Tracker, TrackerChangeSet, TrackerEvent, Util,
};

#[test]
fn test_create_client_returns_a_client() {
    let client = Adb::create_client(Default::default());
    assert_eq!(client.host(), "127.0.0.1");
    assert_eq!(client.port(), 5037);
}

#[test]
fn test_exposes_the_default_types() {
    // Every type that the TypeScript `index.ts` exports must be reachable.
    let _ = ClientOptionsAlias::default();
    let _ = adbkit::KeyCodes::KEYCODE_HOME;
    let _: Option<Stats> = None;
    let _: Option<Loads> = None;
    let _: Option<CpuStats> = None;
    let _: Option<Properties> = None;
    let _: Option<Features> = None;
    let _: Option<FramebufferMeta> = None;
    let _: Option<Forward> = None;
    let _: Option<Reverse> = None;
    let _: Option<TrackerChangeSet> = None;
    let _: Option<ExtendedPublicKey> = None;
}

type ClientOptionsAlias = adbkit::ClientOptions;

#[tokio::test]
async fn test_util_read_all() {
    let out = Util::read_all(&b"FOO"[..]).await.unwrap();
    assert_eq!(out, b"FOO".to_vec());
}

#[test]
fn test_util_parse_public_key() {
    assert!(Util::parse_public_key("not a key").is_err());
    // The auth helper is the one used by the utility.
    assert!(Auth::parse_public_key("not a key").is_err());
}

#[tokio::test]
async fn test_device_client_is_created_from_a_client() {
    let client = Adb::create_client(Default::default());
    let device: DeviceClient = client.get_device("serial");
    assert_eq!(device.serial(), "serial");
}

#[test]
fn test_device_type_parsing() {
    let device = Device::new("serial", DeviceType::parse("offline"));
    assert_eq!(device.r#type, DeviceType::Offline);
    let with_path = DeviceWithPath {
        id: "serial".to_string(),
        r#type: DeviceType::Device,
        path: "usb:1-1".to_string(),
        product: String::new(),
        model: String::new(),
        device: String::new(),
        transport_id: String::new(),
    };
    assert_eq!(with_path.path, "usb:1-1");
}

#[test]
fn test_command_shell_helper() {
    // `ShellCommand.execute()` accepts a plain command or an array of escaped
    // arguments.
    let command = adbkit::command::shell_command(&[
        adbkit::command::Arg::from("getprop"),
        adbkit::command::Arg::from("ro.build.id"),
    ]);
    assert_eq!(command, "'getprop' 'ro.build.id'");
}

/// The stream and long-lived types are part of the public API, so make sure
/// they are all nameable from the crate root.
#[allow(dead_code)]
fn types_are_public() {
    let _: Option<FramebufferStream<tokio::net::TcpStream>> = None;
    let _: Option<ScreenshotStream> = None;
    let _: Option<ShellStream> = None;
    let _: Option<PullTransfer<'static, tokio::net::TcpStream>> = None;
    let _: Option<Sync<tokio::net::TcpStream>> = None;
    let _: Option<Tracker> = None;
    let _: Option<JdwpTracker> = None;
    let _: Option<ProcStat<tokio::net::TcpStream>> = None;
    let _: Option<Parser<tokio::net::TcpStream>> = None;
    let _: Option<PushSource> = None;
    let _: Option<Protocol> = None;
    let _: Option<TrackerEvent> = None;
}
