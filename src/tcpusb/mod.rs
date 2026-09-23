//! An ADB server implemented in Rust.
//!
//! Ported from `src/adb/tcpusb/*.ts`. It is used to share a USB device over TCP
//! (see [`TcpUsbServer`]), and by the `adbkit parse-tcp-packets` command.

pub mod packet;
pub mod packetreader;
pub mod rollingcounter;
pub mod server;
pub mod service;
pub mod servicemap;
pub mod socket;

pub use packet::{Packet, A_AUTH, A_CLSE, A_CNXN, A_OKAY, A_OPEN, A_SYNC, A_WRTE, HEADER_SIZE};
pub use packetreader::PacketReader;
pub use rollingcounter::RollingCounter;
pub use server::{TcpUsbEvent, TcpUsbServer};
pub use service::Service;
pub use servicemap::ServiceMap;
pub use socket::{Socket, SocketConnection};
