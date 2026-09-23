//! The ADB TCP protocol packets.
//!
//! Ported from `src/adb/tcpusb/packet.ts`.

/// `A_SYNC`
pub const A_SYNC: u32 = 0x434e5953;
/// `A_CNXN`
pub const A_CNXN: u32 = 0x4e584e43;
/// `A_OPEN`
pub const A_OPEN: u32 = 0x4e45504f;
/// `A_OKAY`
pub const A_OKAY: u32 = 0x59414b4f;
/// `A_CLSE`
pub const A_CLSE: u32 = 0x45534c43;
/// `A_WRTE`
pub const A_WRTE: u32 = 0x45545257;
/// `A_AUTH`
pub const A_AUTH: u32 = 0x48545541;

/// The size of a packet header.
pub const HEADER_SIZE: usize = 24;

/// A packet of the ADB TCP protocol.
///
/// Ported from the `Packet` class of `src/adb/tcpusb/packet.ts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// The command.
    pub command: u32,
    /// The first argument.
    pub arg0: u32,
    /// The second argument.
    pub arg1: u32,
    /// The length of the payload.
    pub length: u32,
    /// The checksum of the payload.
    pub check: u32,
    /// The magic value, i.e. the command XORed with `0xffffffff`.
    pub magic: u32,
    /// The payload.
    pub data: Option<Vec<u8>>,
}

impl Packet {
    /// Computes the checksum of the given payload.
    ///
    /// Ported from `Packet.checksum()`.
    pub fn checksum(data: Option<&[u8]>) -> u32 {
        match data {
            Some(data) => data.iter().map(|byte| *byte as u32).sum(),
            None => 0,
        }
    }

    /// Computes the magic value for the given command.
    ///
    /// Ported from `Packet.magic()`.
    pub fn magic(command: u32) -> u32 {
        command ^ 0xffff_ffff
    }

    /// Assembles a packet, ready to be written to the wire.
    ///
    /// Ported from `Packet.assemble()`.
    pub fn assemble(command: u32, arg0: u32, arg1: u32, data: Option<&[u8]>) -> Vec<u8> {
        let payload = data.unwrap_or(&[]);
        let mut chunk = Vec::with_capacity(HEADER_SIZE + payload.len());
        chunk.extend_from_slice(&command.to_le_bytes());
        chunk.extend_from_slice(&arg0.to_le_bytes());
        chunk.extend_from_slice(&arg1.to_le_bytes());
        chunk.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&Packet::checksum(Some(payload)).to_le_bytes());
        chunk.extend_from_slice(&Packet::magic(command).to_le_bytes());
        chunk.extend_from_slice(payload);
        chunk
    }

    /// Swaps the endianness of a 32 bit integer.
    ///
    /// Ported from `Packet.swap32()`.
    pub fn swap32(value: u32) -> u32 {
        value.swap_bytes()
    }

    /// Whether the checksum of the payload matches.
    ///
    /// Ported from `Packet.verifyChecksum()`.
    pub fn verify_checksum(&self) -> bool {
        self.check == 0 || self.check == Packet::checksum(self.data.as_deref())
    }

    /// Whether the magic value matches.
    ///
    /// Ported from `Packet.verifyMagic()`.
    pub fn verify_magic(&self) -> bool {
        self.magic == Packet::magic(self.command)
    }

    /// The name of the command.
    pub fn command_name(&self) -> Option<&'static str> {
        Some(match self.command {
            A_SYNC => "SYNC",
            A_CNXN => "CNXN",
            A_OPEN => "OPEN",
            A_OKAY => "OKAY",
            A_CLSE => "CLSE",
            A_WRTE => "WRTE",
            A_AUTH => "AUTH",
            _ => return None,
        })
    }
}

impl std::fmt::Display for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} arg0={} arg1={} length={}",
            self.command_name().unwrap_or("UNKNOWN"),
            self.arg0,
            self.arg1,
            self.length
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_codes() {
        assert_eq!(A_SYNC, 0x434e5953);
        assert_eq!(A_CNXN, 0x4e584e43);
        assert_eq!(A_OPEN, 0x4e45504f);
        assert_eq!(A_OKAY, 0x59414b4f);
        assert_eq!(A_CLSE, 0x45534c43);
        assert_eq!(A_WRTE, 0x45545257);
        assert_eq!(A_AUTH, 0x48545541);
    }

    #[test]
    fn test_checksum() {
        assert_eq!(Packet::checksum(None), 0);
        assert_eq!(Packet::checksum(Some(&[1, 2, 3])), 6);
    }

    #[test]
    fn test_magic() {
        assert_eq!(Packet::magic(A_CNXN), A_CNXN ^ 0xffff_ffff);
    }

    #[test]
    fn test_assemble_without_payload() {
        let packet = Packet::assemble(A_OKAY, 1, 2, None);
        assert_eq!(packet.len(), HEADER_SIZE);
        assert_eq!(u32::from_le_bytes(packet[0..4].try_into().unwrap()), A_OKAY);
        assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(packet[12..16].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(packet[16..20].try_into().unwrap()), 0);
        assert_eq!(
            u32::from_le_bytes(packet[20..24].try_into().unwrap()),
            Packet::magic(A_OKAY)
        );
    }

    #[test]
    fn test_assemble_with_payload() {
        let data = b"hello";
        let packet = Packet::assemble(A_WRTE, 1, 2, Some(data));
        assert_eq!(packet.len(), HEADER_SIZE + data.len());
        assert_eq!(u32::from_le_bytes(packet[12..16].try_into().unwrap()), 5);
        assert_eq!(u32::from_le_bytes(packet[16..20].try_into().unwrap()), 532);
        assert_eq!(&packet[HEADER_SIZE..], data);
    }

    #[test]
    fn test_swap32() {
        assert_eq!(Packet::swap32(1), 0x0100_0000);
        assert_eq!(Packet::swap32(0x0102_0304), 0x0403_0201);
    }

    #[test]
    fn test_display() {
        let packet = Packet {
            command: A_CNXN,
            arg0: 1,
            arg1: 2,
            length: 0,
            check: 0,
            magic: Packet::magic(A_CNXN),
            data: None,
        };
        assert_eq!(packet.to_string(), "CNXN arg0=1 arg1=2 length=0");
        assert_eq!(packet.command_name(), Some("CNXN"));
    }
}
