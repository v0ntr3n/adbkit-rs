//! Tests for the device commands.
//!
//! Ported from `test/adb/command/host-serial/*.ts` and
//! `test/adb/command/host-transport/*.ts`.

mod common;

use adbkit::command::{host_serial, host_transport};
use adbkit::parser::Parser;

#[tokio::test]
async fn test_attach_and_detach() {
    for attach in [true, false] {
        let (mut mock, mut conn) = common::mock();
        mock.feed_okay().await;
        mock.end().await;

        let ok = if attach {
            host_serial::attach(&mut conn, "serial").await.unwrap()
        } else {
            host_serial::detach(&mut conn, "serial").await.unwrap()
        };
        assert!(ok);
        common::assert_written(
            &mut mock,
            if attach {
                "host-serial:serial:attach"
            } else {
                "host-serial:serial:detach"
            },
        )
        .await;
    }
}

#[tokio::test]
async fn test_forward() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_okay().await;
    mock.end().await;

    assert!(
        host_serial::forward(&mut conn, "serial", "tcp:1234", "localabstract:foo")
            .await
            .unwrap()
    );
    common::assert_written(
        &mut mock,
        "host-serial:serial:forward:tcp:1234;localabstract:foo",
    )
    .await;
}

#[tokio::test]
async fn test_get_device_path() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"usb:1-1").await;
    mock.end().await;

    let path = host_serial::get_device_path(&mut conn, "serial")
        .await
        .unwrap();
    assert_eq!(path, "usb:1-1");
    common::assert_written(&mut mock, "host-serial:serial:get-devpath").await;
}

#[tokio::test]
async fn test_get_serial_no() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"serial").await;
    mock.end().await;

    assert_eq!(
        host_serial::get_serial_no(&mut conn, "serial")
            .await
            .unwrap(),
        "serial"
    );
    common::assert_written(&mut mock, "host-serial:serial:get-serialno").await;
}

#[tokio::test]
async fn test_get_state() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"device").await;
    mock.end().await;

    assert_eq!(
        host_serial::get_state(&mut conn, "serial").await.unwrap(),
        "device"
    );
    common::assert_written(&mut mock, "host-serial:serial:get-state").await;
}

#[tokio::test]
async fn test_list_forwards() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"serial tcp:1234 localabstract:foo\n")
        .await;
    mock.end().await;

    let forwards = host_serial::list_forwards(&mut conn, "serial")
        .await
        .unwrap();
    assert_eq!(forwards.len(), 1);
    assert_eq!(forwards[0].serial, "serial");
    assert_eq!(forwards[0].local, "tcp:1234");
    assert_eq!(forwards[0].remote, "localabstract:foo");
}

#[tokio::test]
async fn test_wait_for_device() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_okay().await;
    mock.end().await;

    assert_eq!(
        host_serial::wait_for_device(&mut conn, "serial")
            .await
            .unwrap(),
        "serial"
    );
    common::assert_written(&mut mock, "host-serial:serial:wait-for-any-device").await;
}

#[tokio::test]
async fn test_clear() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Success\r\n").await;
    mock.end().await;

    assert!(host_transport::clear(&mut conn, "com.example")
        .await
        .unwrap());
    common::assert_written(&mut mock, "shell:pm clear com.example").await;
}

#[tokio::test]
async fn test_clear_failed() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Failed\r\n").await;
    mock.end().await;

    let err = host_transport::clear(&mut conn, "com.example")
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Package 'com.example' could not be cleared"
    );
}

#[tokio::test]
async fn test_get_features() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"feature:android.hardware.nfc\r\nfeature:reqGlEsVersion=0x20000\r\n")
        .await;
    mock.end().await;

    let features = host_transport::get_features(&mut conn).await.unwrap();
    assert_eq!(features.get("android.hardware.nfc"), Some(&true));
    assert_eq!(features.get("reqGlEsVersion"), Some(&true));
    common::assert_written(&mut mock, "shell:pm list features 2>/dev/null").await;
}

#[tokio::test]
async fn test_get_packages() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"package:com.android.settings\r\npackage:com.foo\r\n")
        .await;
    mock.end().await;

    let packages = host_transport::get_packages(&mut conn, None).await.unwrap();
    assert_eq!(packages, vec!["com.android.settings", "com.foo"]);
    common::assert_written(&mut mock, "shell:pm list packages 2>/dev/null").await;
}

#[tokio::test]
async fn test_get_packages_with_flags() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"package:com.foo\r\n").await;
    mock.end().await;

    host_transport::get_packages(&mut conn, Some("-3"))
        .await
        .unwrap();
    common::assert_written(&mut mock, "shell:pm list packages -3 2>/dev/null").await;
}

#[tokio::test]
async fn test_get_properties() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"[ro.product.model]: [Pixel 7]\r\n").await;
    mock.end().await;

    let properties = host_transport::get_properties(&mut conn).await.unwrap();
    assert_eq!(properties.get("ro.product.model").unwrap(), "Pixel 7");
    common::assert_written(&mut mock, "shell:getprop").await;
}

#[tokio::test]
async fn test_install() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Success\r\n").await;
    mock.end().await;

    assert!(host_transport::install(&mut conn, "foo").await.unwrap());
    common::assert_written(&mut mock, "shell:pm install -r \"foo\"").await;
}

#[tokio::test]
async fn test_install_failure() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Failure [ALREADY_EXISTS]\r\n").await;
    mock.end().await;

    let err = host_transport::install(&mut conn, "foo").await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "foo could not be installed [ALREADY_EXISTS]"
    );
    match err {
        adbkit::Error::Install { code, .. } => assert_eq!(code, "ALREADY_EXISTS"),
        other => panic!("expected an install error, got {:?}", other),
    }
}

#[tokio::test]
async fn test_install_ignores_other_data() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"open: Permission failed\r\n").await;
    mock.feed(b"Success\r\n").await;
    mock.end().await;

    assert!(host_transport::install(&mut conn, "foo").await.unwrap());
}

#[tokio::test]
async fn test_is_installed() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"package:foo\r\n").await;
    mock.end().await;

    assert!(host_transport::is_installed(&mut conn, "foo")
        .await
        .unwrap());
    common::assert_written(&mut mock, "shell:pm path foo 2>/dev/null").await;
}

#[tokio::test]
async fn test_is_installed_not_found() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    assert!(!host_transport::is_installed(&mut conn, "foo")
        .await
        .unwrap());
}

#[tokio::test]
async fn test_is_installed_unexpected() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"open: Permission failed\r\n").await;
    mock.end().await;

    let err = host_transport::is_installed(&mut conn, "foo")
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected 'open: Pe', was expecting 'package:'"
    );
}

#[tokio::test]
async fn test_list_reverses() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"serial tcp:1234 localabstract:foo\n")
        .await;
    mock.end().await;

    let reverses = host_transport::list_reverses(&mut conn).await.unwrap();
    assert_eq!(reverses.len(), 1);
    assert_eq!(reverses[0].remote, "tcp:1234");
    assert_eq!(reverses[0].local, "localabstract:foo");
    common::assert_written(&mut mock, "reverse:list-forward").await;
}

#[tokio::test]
async fn test_open_local() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::open_local(conn, "/dev/socket/foo")
        .await
        .unwrap();
    common::assert_written(&mut mock, "localfilesystem:/dev/socket/foo").await;
}

#[tokio::test]
async fn test_open_local_with_scheme() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::open_local(conn, "localabstract:foo")
        .await
        .unwrap();
    common::assert_written(&mut mock, "localabstract:foo").await;
}

#[tokio::test]
async fn test_open_log() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::open_log(conn, "main").await.unwrap();
    common::assert_written(&mut mock, "log:main").await;
}

#[tokio::test]
async fn test_logcat() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::logcat(conn, false).await.unwrap();
    common::assert_written(&mut mock, "shell:echo && logcat -B *:I 2>/dev/null").await;
}

#[tokio::test]
async fn test_logcat_clear() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::logcat(conn, true).await.unwrap();
    common::assert_written(
        &mut mock,
        "shell:echo && logcat -c 2>/dev/null && logcat -B *:I 2>/dev/null",
    )
    .await;
}

#[tokio::test]
async fn test_logcat_performs_crlf_transformation() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"\r\nfoo\r\n").await;
    mock.end().await;

    let stream = host_transport::logcat(conn, false).await.unwrap();
    assert_eq!(common::read_all(stream).await.unwrap(), b"foo\n".to_vec());
}

#[tokio::test]
async fn test_monkey() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b":Monkey: foo\n").await;
    mock.feed(b"more output\n").await;
    mock.end().await;

    host_transport::monkey(conn, 1080).await.unwrap();
    common::assert_written(
        &mut mock,
        "shell:EXTERNAL_STORAGE=/data/local/tmp monkey --port 1080 -v",
    )
    .await;
}

#[tokio::test]
async fn test_monkey_timeout_falls_back() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    // No banner at all: the 1 second timeout has to kick in.
    let task = tokio::spawn(async move { host_transport::monkey(conn, 1080).await });

    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    mock.end().await;
    let stream = task.await.unwrap().unwrap();
    common::assert_written(
        &mut mock,
        "shell:EXTERNAL_STORAGE=/data/local/tmp monkey --port 1080 -v",
    )
    .await;
    drop(stream);
}

#[tokio::test]
async fn test_reboot() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"").await;
    mock.end().await;

    assert!(host_transport::reboot(&mut conn).await.unwrap());
    common::assert_written(&mut mock, "reboot:").await;
}

#[tokio::test]
async fn test_remount() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    assert!(host_transport::remount(&mut conn).await.unwrap());
    common::assert_written(&mut mock, "remount:").await;
}

#[tokio::test]
async fn test_reverse() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_okay().await;
    mock.end().await;

    assert!(
        host_transport::reverse(&mut conn, "tcp:1234", "localabstract:foo")
            .await
            .unwrap()
    );
    common::assert_written(&mut mock, "reverse:forward:tcp:1234;localabstract:foo").await;
}

#[tokio::test]
async fn test_root() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"restarting adbd as root").await;
    mock.end().await;

    assert!(host_transport::root(&mut conn).await.unwrap());
    common::assert_written(&mut mock, "root:").await;
}

#[tokio::test]
async fn test_root_unexpected() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"adbd cannot run as root in production builds")
        .await;
    mock.end().await;

    let err = host_transport::root(&mut conn).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "adbd cannot run as root in production builds"
    );
}

#[tokio::test]
async fn test_screencap() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"\r\nlegit image").await;
    mock.end().await;

    let stream = host_transport::screencap(conn).await.unwrap();
    assert_eq!(
        common::read_all(stream).await.unwrap(),
        b"legit image".to_vec()
    );
    common::assert_written(&mut mock, "shell:echo && screencap -p 2>/dev/null").await;
}

#[tokio::test]
async fn test_screencap_not_supported() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    let err = host_transport::screencap(conn).await.unwrap_err();
    assert_eq!(err.to_string(), "No support for the screencap command");
}

#[tokio::test]
async fn test_screencap_performs_crlf_transformation() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"\r\nfoo\r\n").await;
    mock.end().await;

    let stream = host_transport::screencap(conn).await.unwrap();
    assert_eq!(common::read_all(stream).await.unwrap(), b"foo\n".to_vec());
}

#[tokio::test]
async fn test_screencap_without_transformation() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"\nfoo\r\n").await;
    mock.end().await;

    let stream = host_transport::screencap(conn).await.unwrap();
    assert_eq!(common::read_all(stream).await.unwrap(), b"foo\r\n".to_vec());
}

#[tokio::test]
async fn test_shell_string_command() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::shell(conn, "foo 'bar").await.unwrap();
    common::assert_written(&mut mock, "shell:foo 'bar").await;
}

#[tokio::test]
async fn test_shell_array_command() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    let command = host_transport::shell_command(&[
        adbkit::command::Arg::from("foo"),
        adbkit::command::Arg::from("'bar'"),
        adbkit::command::Arg::from("\""),
    ]);
    host_transport::shell(conn, &command).await.unwrap();
    common::assert_written(&mut mock, "shell:'foo' ''\"'\"'bar'\"'\"'' '\"'").await;
}

#[tokio::test]
async fn test_shell_does_not_escape_numbers() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    let command = host_transport::shell_command(&[
        adbkit::command::Arg::from("foo"),
        adbkit::command::Arg::from(67u32),
    ]);
    host_transport::shell(conn, &command).await.unwrap();
    common::assert_written(&mut mock, "shell:'foo' 67").await;
}

#[tokio::test]
async fn test_shell_fail() {
    let (mut mock, conn) = common::mock();
    mock.feed_fail("mystery").await;
    mock.end().await;

    let err = host_transport::shell(conn, "foo").await.unwrap_err();
    assert_eq!(err.to_string(), "Failure: 'mystery'");
}

#[tokio::test]
async fn test_sync() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    let _sync = host_transport::sync_service(conn).await.unwrap();
    common::assert_written(&mut mock, "sync:").await;
}

#[tokio::test]
async fn test_open_tcp() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::open_tcp(conn, 1234, None).await.unwrap();
    common::assert_written(&mut mock, "tcp:1234").await;
}

#[tokio::test]
async fn test_open_tcp_with_host() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.end().await;

    host_transport::open_tcp(conn, 1234, Some("example.com"))
        .await
        .unwrap();
    common::assert_written(&mut mock, "tcp:1234:example.com").await;
}

#[tokio::test]
async fn test_tcpip() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"restarting in TCP mode port: 5555").await;
    mock.end().await;

    assert_eq!(host_transport::tcpip(&mut conn, 5555).await.unwrap(), 5555);
    common::assert_written(&mut mock, "tcpip:5555").await;
}

#[tokio::test]
async fn test_tcpip_unexpected() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"more than one device/emulator").await;
    mock.end().await;

    let err = host_transport::tcpip(&mut conn, 5555).await.unwrap_err();
    assert_eq!(err.to_string(), "more than one device/emulator");
}

#[tokio::test]
async fn test_usb() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"restarting in USB mode").await;
    mock.end().await;

    assert!(host_transport::usb(&mut conn).await.unwrap());
    common::assert_written(&mut mock, "usb:").await;
}

#[tokio::test]
async fn test_wait_boot_complete() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"1\r\n").await;
    mock.end().await;

    assert!(host_transport::wait_boot_complete(&mut conn).await.unwrap());
    common::assert_written(
        &mut mock,
        "shell:while getprop sys.boot_completed 2>/dev/null; do sleep 1; done",
    )
    .await;
}

#[tokio::test]
async fn test_uninstall() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Success\r\n").await;
    mock.end().await;

    assert!(host_transport::uninstall(&mut conn, "com.example")
        .await
        .unwrap());
    common::assert_written(&mut mock, "shell:pm uninstall com.example").await;
}

#[tokio::test]
async fn test_uninstall_unknown_package_succeeds() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Unknown package: com.example\r\n").await;
    mock.end().await;

    assert!(host_transport::uninstall(&mut conn, "com.example")
        .await
        .unwrap());
}

#[tokio::test]
async fn test_uninstall_policy_manager() {
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Failure [DELETE_FAILED_DEVICE_POLICY_MANAGER]\r\n")
        .await;
    mock.end().await;

    let err = host_transport::uninstall(&mut conn, "com.example")
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "com.example could not be uninstalled [Failure [DELETE_FAILED_DEVICE_POLICY_MANAGER]]"
    );
}

#[tokio::test]
async fn test_framebuffer() {
    let (mut mock, conn) = common::mock();
    let mut meta = vec![0u8; 52];
    let write = |meta: &mut Vec<u8>, offset: usize, value: u32| {
        meta[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };
    write(&mut meta, 0, 1); // version
    write(&mut meta, 4, 32); // bpp
    write(&mut meta, 8, 819200); // size
    write(&mut meta, 12, 640); // width
    write(&mut meta, 16, 320); // height
    write(&mut meta, 20, 0); // red_offset
    write(&mut meta, 24, 8); // red_length
    write(&mut meta, 28, 16); // blue_offset
    write(&mut meta, 32, 8); // blue_length
    write(&mut meta, 36, 8); // green_offset
    write(&mut meta, 40, 8); // green_length
    write(&mut meta, 44, 24); // alpha_offset
    write(&mut meta, 48, 8); // alpha_length
    mock.feed_okay().await;
    mock.feed(&meta).await;
    mock.end().await;

    let stream = host_transport::framebuffer(conn, "raw").await.unwrap();

    common::assert_written(&mut mock, "framebuffer:").await;
    assert_eq!(stream.meta.version, 1);
    assert_eq!(stream.meta.bpp, 32);
    assert_eq!(stream.meta.size, 819200);
    assert_eq!(stream.meta.width, 640);
    assert_eq!(stream.meta.height, 320);
    assert_eq!(stream.meta.red_offset, 0);
    assert_eq!(stream.meta.format, adbkit::ColorFormat::Rgba);
}

#[tokio::test]
async fn test_framebuffer_raw_stream() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(&[0u8; 52]).await;
    mock.feed(b"raw data").await;
    mock.end().await;

    let stream = host_transport::framebuffer(conn, "raw").await.unwrap();
    assert_eq!(
        common::read_all(stream).await.unwrap(),
        b"raw data".to_vec()
    );
}

#[tokio::test]
async fn test_start_activity() {
    use adbkit::{StartActivityOptions, StartServiceOptions};

    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Starting: Intent { }\r\n").await;
    mock.end().await;

    let service = StartServiceOptions {
        action: Some("android.intent.action.VIEW".to_string()),
        ..Default::default()
    };
    let options = StartActivityOptions {
        service,
        ..Default::default()
    };
    assert!(host_transport::start_activity(&mut conn, &options)
        .await
        .unwrap());
    common::assert_written(&mut mock, "shell:am start -a 'android.intent.action.VIEW'").await;
}

#[tokio::test]
async fn test_start_activity_error() {
    use adbkit::{StartActivityOptions, StartServiceOptions};

    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Error: Bad parameters\r\n").await;
    mock.end().await;

    let options = StartActivityOptions {
        service: StartServiceOptions::default(),
        ..Default::default()
    };
    let err = host_transport::start_activity(&mut conn, &options)
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "Bad parameters");
}

#[tokio::test]
async fn test_start_activity_debug_and_wait() {
    use adbkit::StartActivityOptions;

    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Starting: Intent { }\r\n").await;
    mock.end().await;

    let options = StartActivityOptions {
        debug: true,
        wait: true,
        ..Default::default()
    };
    assert!(host_transport::start_activity(&mut conn, &options)
        .await
        .unwrap());
    common::assert_written(&mut mock, "shell:am start -D -W").await;
}

#[tokio::test]
async fn test_start_service() {
    use adbkit::StartServiceOptions as ServiceOptions;

    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Starting service: Intent { }\r\n").await;
    mock.end().await;

    let options = ServiceOptions {
        component: Some("com.example/.Service".to_string()),
        ..Default::default()
    };
    assert!(host_transport::start_service(&mut conn, &options)
        .await
        .unwrap());
    common::assert_written(&mut mock, "shell:am startservice -n 'com.example/.Service'").await;
}

#[tokio::test]
async fn test_start_service_retries_without_user() {
    // When the device does not support `--user`, the command fails and has to
    // be retried without it.
    let (mut mock, mut conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"Error: Unknown option: --user\r\n").await;
    mock.end().await;

    let options = adbkit::StartServiceOptions {
        user: Some(10),
        ..Default::default()
    };
    let err = host_transport::start_service(&mut conn, &options)
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "Unknown option: --user");
    common::assert_written(&mut mock, "shell:am startservice --user '10'").await;
}

#[tokio::test]
async fn test_track_jdwp() {
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed_value(b"1234\n5678").await;
    mock.end().await;

    let mut tracker = host_transport::track_jdwp(conn).await.unwrap();

    common::assert_written(&mut mock, "track-jdwp").await;
    let event = tracker.next().await.expect("an event");
    match event {
        adbkit::JdwpTrackerEvent::ChangeSet(changes, pids) => {
            assert_eq!(changes.added, vec!["1234", "5678"]);
            assert_eq!(pids, vec!["1234", "5678"]);
        }
        other => panic!("unexpected event: {:?}", other),
    }
}

#[tokio::test]
async fn test_parser_passthrough_keeps_buffered_bytes() {
    // A command that returns the raw stream must not lose data that the parser
    // had already buffered.
    let (mut mock, conn) = common::mock();
    mock.feed_okay().await;
    mock.feed(b"payload").await;
    mock.end().await;

    let stream = host_transport::shell(conn, "foo").await.unwrap();
    assert_eq!(common::read_all(stream).await.unwrap(), b"payload".to_vec());
    assert!(!Parser::new(tokio::io::empty(), None).is_ended());
}
