//! The `adbkit` command line tool.
//!
//! Ported from `src/cli.ts`.

use std::io::Write;
use std::path::PathBuf;

use adbkit::adb::Adb;
use adbkit::auth::Auth;
use adbkit::tcpusb::PacketReader;
use clap::{Parser as ClapParser, Subcommand};

#[derive(ClapParser, Debug)]
#[command(
    name = "adbkit",
    version,
    about = "A client for the Android Debug Bridge"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Converts an ADB-generated public key into PEM format.
    PubkeyConvert {
        /// The file to convert.
        file: PathBuf,
        /// The format (pem or openssh).
        #[arg(short, long, default_value = "pem")]
        format: String,
    },
    /// Outputs the fingerprint of an ADB-generated public key.
    PubkeyFingerprint {
        /// The file to read.
        file: PathBuf,
    },
    /// Provides an USB device over TCP using a translating proxy.
    UsbDeviceToTcp {
        /// The serial of the device to share.
        serial: String,
        /// The port number to listen on.
        #[arg(short, long, default_value = "6174")]
        port: u16,
    },
    /// Parses ADB TCP packets from the given file.
    ParseTcpPackets {
        /// The file to read.
        file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    adbkit::init_logging();

    let cli = Cli::parse();
    match cli.command {
        Command::PubkeyConvert { file, format } => {
            let contents = std::fs::read_to_string(&file)?;
            let key = Auth::parse_public_key(&contents)?;
            let output = match format.to_lowercase().as_str() {
                "pem" => key.to_pem(),
                "openssh" => key.to_openssh("adbkey"),
                other => {
                    eprintln!("Unsupported format '{}'", other);
                    std::process::exit(1);
                }
            };
            // Writing to a locked handle avoids a panic when the output is
            // piped into something that closes early.
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{}", output.trim());
        }
        Command::PubkeyFingerprint { file } => {
            let contents = std::fs::read_to_string(&file)?;
            let key = Auth::parse_public_key(&contents)?;
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{} {}", key.fingerprint, key.comment);
        }
        Command::UsbDeviceToTcp { serial, port } => {
            let client = Adb::create_client(Default::default());
            let mut bridge = client.create_tcp_usb_bridge(&serial, Default::default());
            let mut events = bridge.events();
            bridge.listen(port).await?;
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "Connect with `adb connect localhost:{}`", port);
            while let Some(event) = events.recv().await {
                match event {
                    adbkit::tcpusb::TcpUsbEvent::Error(err) => {
                        eprintln!("An error occured: {}", err);
                    }
                    adbkit::tcpusb::TcpUsbEvent::Close => break,
                    _ => {}
                }
            }
        }
        Command::ParseTcpPackets { file } => {
            let file = tokio::fs::File::open(&file).await?;
            let stdout = std::io::stdout();
            let mut stdout = stdout.lock();
            let mut reader = PacketReader::new(file);
            while let Some(packet) = reader.next().await {
                match packet {
                    Ok(packet) => {
                        let _ = writeln!(stdout, "{}", packet);
                    }
                    Err(err) => {
                        eprintln!("{}", err);
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    Ok(())
}
