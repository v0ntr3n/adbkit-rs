# adbkit (Rust)

A pure Rust client for the [Android Debug Bridge][adb] server, ported from the
TypeScript [`@devicefarmer/adbkit`][ts] library. It can be used either as a
library in your own application, or simply as a convenient utility for playing
with your device.

Most of the `adb` command line tool's functionality is supported (including
pushing/pulling files, installing APKs and processing logs), with some added
functionality such as being able to generate touch/key events and take
screenshots.

> **Note**: like the original, this project is **not** an implementation of the
> ADB _server_. The target host (where the devices are connected) must still
> have ADB installed and either already running (e.g. via `adb start-server`) or
> available in `$PATH`. An attempt will be made to start the server locally via
> `adb start-server` if the initial connection fails. This is the only case
> where we fall back to the `adb` binary. (A TCP-USB *bridge*, which lets you
> share a USB device over TCP, is also included.)

Throughout this documentation — and in the doc comments of the source — file
paths such as `src/adb/parser.ts` refer to files of the original TypeScript
project, so that every module here can be compared with the one it was ported
from.

## Requirements

-   Rust 1.75 or later.
-   The `adb` command line tool.
-   GraphicsMagick (`gm`) if you want `framebuffer()` to convert to a format
    such as PNG.

## Installation

```toml
[dependencies]
adbkit = "3.3"
tokio = { version = "1", features = ["full"] }
```

Logging follows the original's `DEBUG` environment variable (or `RUST_LOG`):

```bash
DEBUG=adb:* cargo run --example list_devices
```

## Examples

#### Checking for NFC support

```rust,no_run
use adbkit::Adb;

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    let devices = client.list_devices().await?;
    for device in devices {
        let features = client.get_device(&device.id).get_features().await?;
        if features.get("android.hardware.nfc").copied().unwrap_or(false) {
            println!("{} supports NFC", device.id);
        }
    }
    Ok(())
}
```

#### Installing an APK

```rust,no_run
use adbkit::Adb;

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    let apk = "vendor/app.apk";
    for device in client.list_devices().await? {
        client.get_device(&device.id).install(apk).await?;
    }
    println!("Installed {} on all connected devices", apk);
    Ok(())
}
```

#### Tracking devices

```rust,no_run
use adbkit::Adb;
use adbkit::TrackerEvent;

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    let mut tracker = client.track_devices().await?;
    while let Some(event) = tracker.next().await {
        match event {
            TrackerEvent::Add(device) => println!("Device {} was plugged in", device.id),
            TrackerEvent::Remove(device) => println!("Device {} was unplugged", device.id),
            TrackerEvent::End => break,
            _ => {}
        }
    }
    Ok(())
}
```

#### Pulling a file from all connected devices

```rust,no_run
use adbkit::{Adb, PullOptions};

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    for device in client.list_devices().await? {
        let mut options = PullOptions::default();
        options.progress = Some(Box::new(|bytes| {
            print!("\r[{}] Pulled {} bytes so far", device.id, bytes);
        }));
        let transfer = client
            .get_device(&device.id)
            .pull("/system/build.prop", options)
            .await?;
        let data = transfer.read_to_vec().await?;
        println!("Pulled {} bytes", data.len());
    }
    Ok(())
}
```

#### Pushing a file to a device

```rust,no_run
use adbkit::{Adb, PushOptions};

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    let mut options = PushOptions::default();
    options.progress = Some(Box::new(|bytes| println!("Pushed {} bytes", bytes)));
    let pushed = client
        .get_device("ZX1G22K9QM")
        .push("temp/foo.txt".into(), "/data/local/tmp/foo.txt", None, options)
        .await?;
    println!("Pushed {} bytes", pushed);
    Ok(())
}
```

#### Listing files in a folder

```rust,no_run
use adbkit::Adb;

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    for device in client.list_devices().await? {
        for file in client.get_device(&device.id).readdir("/sdcard").await? {
            if file.is_file() {
                println!("[{}] Found file \"{}\"", device.id, file.name);
            }
        }
    }
    Ok(())
}
```

#### Taking a screenshot

```rust,no_run
use adbkit::Adb;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    let device = client.get_device("ZX1G22K9QM");
    let mut png = Vec::new();
    device.screencap().await?.read_to_end(&mut png).await?;
    std::fs::write("screenshot.png", png)?;
    Ok(())
}
```

#### Monkey

```rust,no_run
use adbkit::Adb;

#[tokio::main]
async fn main() -> Result<(), adbkit::Error> {
    let client = Adb::create_client(Default::default());
    let monkey = client.get_device("ZX1G22K9QM").open_monkey(None).await?;
    monkey.tap(100, 200).await?;
    Ok(())
}
```

## Command line

The crate also installs the `adbkit` binary, mirroring `src/cli.ts`:

```bash
# Convert an ADB public key into PEM (or OpenSSH) format
adbkit pubkey-convert ~/.android/adbkey.pub
adbkit pubkey-convert ~/.android/adbkey.pub --format openssh

# Print the fingerprint of an ADB public key
adbkit pubkey-fingerprint ~/.android/adbkey.pub

# Share a USB device over TCP
adbkit usb-device-to-tcp ZX1G22K9QM --port 6174

# Dump ADB TCP packets from a capture file
adbkit parse-tcp-packets tcp.dump
```

## Feature parity

Every module of the TypeScript library has a counterpart here:

| TypeScript | Rust |
| --- | --- |
| `Adb` (`src/adb.ts`) | [`adbkit::Adb`] |
| `Client` (`src/adb/client.ts`) | [`adbkit::Client`] |
| `DeviceClient` (`src/adb/DeviceClient.ts`) | [`adbkit::DeviceClient`] |
| `Connection` | [`adbkit::Connection`] |
| `Parser` | [`adbkit::Parser`] |
| `Protocol` | [`adbkit::Protocol`] |
| `Sync` | [`adbkit::Sync`] |
| `Tracker` | [`adbkit::TrackerHandle`] |
| `JdwpTracker` | [`adbkit::JdwpTracker`] |
| `ProcStat` | [`adbkit::ProcStat`] |
| `Auth` | [`adbkit::Auth`] |
| `LineTransform` | [`adbkit::LineTransform`] |
| `RgbTransform` | [`adbkit::framebuffer::RgbTransform`] |
| `tcpusb/*` | [`adbkit::tcpusb`] |
| `adbkit-logcat` | [`adbkit::logcat`] |
| `adbkit-monkey` | [`adbkit::monkey`] |

Differences from the TypeScript API, all deliberate:

-   The API is `async`; Node's callbacks, promises and `EventEmitter`s are
    replaced by `async`/`await` and channels/streams.
-   `client.disconnect()` resolves with the device ID (the TypeScript version
    returns a `DeviceClient`, which the documentation describes as the ID).
-   Transfer progress is reported through an optional callback
    (`PushOptions`/`PullOptions`) and `bytes_transferred()`, instead of
    `'progress'` events.

## Tests

```bash
cargo test
```

The mocha suite of the original has been ported to Rust unit and integration
tests, including the protocol/parser/line-transform/RGB-transform tests and the
mock-connection tests for every command (using `tokio::io::duplex` in place of
Node's mock duplex streams).

Tests that require a physical device are marked `#[ignore]` and can be run with
`cargo test -- --ignored` while a device is connected (they talk to a real ADB
server).

[adb]: https://developer.android.com/studio/command-line/adb
[ts]: https://github.com/devicefarmer/adbkit
