use tokio::io::{self, AsyncReadExt};

use crate::protocol::{Packet, publish::PublishPacket};

use super::{encode_remaining_length, read_remaining_length};

/// MQTT PUBACK packet
///
/// Sent in response to a PUBLISH packet with QoS 1.
/// Acknowledges receipt of the message.
///
/// Format:
/// - Fixed Header: packet type (4) + reserved flags (0000)
/// - Variable Header: packet identifier (matching the PUBLISH)
/// - Payload: none
#[derive(Debug, Clone)]
pub struct PubackPacket {
    /// Packet identifier (must match the PUBLISH packet)
    pub packet_id: u16,
}

impl Packet for PubackPacket {
    /// Read a PUBACK packet from a stream
    /// Assumes the fixed header byte has already been read and is passed in
    async fn read<R: AsyncReadExt + Unpin>(stream: &mut R, fixed_header: u8) -> io::Result<Self> {
        let packet_type = fixed_header >> 4;
        if packet_type != 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Expected PUBACK packet (type 4), got type {}", packet_type),
            ));
        }

        // Check reserved flags (must be 0000 for PUBACK)
        let flags = fixed_header & 0x0F;
        if flags != 0b0000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid PUBACK flags: expected 0000, got {:04b}", flags),
            ));
        }

        // Read remaining length (should be 2)
        let remaining_length = read_remaining_length(stream).await?;
        if remaining_length != 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid PUBACK remaining length: expected 2, got {}", remaining_length),
            ));
        }

        // Read packet identifier (2 bytes)
        let packet_id = stream.read_u16().await?;

        Ok(PubackPacket { packet_id })
    }

    /// Encode the PUBACK packet to bytes for sending
    fn encode(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        // Fixed header byte: packet type (4) + reserved flags (0000)
        buffer.push(0x40); // 0100_0000

        // Remaining length is always 2 (just the packet identifier)
        encode_remaining_length(&mut buffer, 2);

        // Packet identifier (2 bytes, big-endian)
        buffer.push((self.packet_id >> 8) as u8);
        buffer.push((self.packet_id & 0xFF) as u8);

        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_encode_puback() {
        let packet = PubackPacket { packet_id: 42 };
        let encoded = packet.encode();

        // Check packet type and flags
        assert_eq!(encoded[0], 0x40); // 0100_0000

        // Check remaining length (always 2)
        assert_eq!(encoded[1], 0x02);

        // Check packet ID
        assert_eq!(encoded[2], 0x00);
        assert_eq!(encoded[3], 0x2A); // 42
    }

    #[tokio::test]
    async fn test_read_puback() {
        // PUBACK packet with packet_id=123
        let data = vec![
            0x40, // Fixed header: PUBACK with flags 0000
            0x02, // Remaining length: 2
            0x00, 0x7B, // Packet ID: 123
        ];

        let mut cursor = Cursor::new(data);
        let header = cursor.read_u8().await.unwrap();
        let packet = PubackPacket::read(&mut cursor, header).await.unwrap();

        assert_eq!(packet.packet_id, 123);
    }

    #[tokio::test]
    async fn test_read_puback_invalid_type() {
        // Wrong packet type (PUBLISH instead of PUBACK)
        let data = vec![
            0x30, // Fixed header: PUBLISH
            0x02, // Remaining length: 2
            0x00, 0x01, // Packet ID: 1
        ];

        let mut cursor = Cursor::new(data);
        let header = cursor.read_u8().await.unwrap();
        let result = PubackPacket::read(&mut cursor, header).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_puback_invalid_length() {
        // Invalid remaining length (should be 2)
        let data = vec![
            0x40, // Fixed header: PUBACK
            0x03, // Remaining length: 3 (invalid!)
            0x00, 0x01, 0x00,
        ];

        let mut cursor = Cursor::new(data);
        let header = cursor.read_u8().await.unwrap();
        let result = PubackPacket::read(&mut cursor, header).await;

        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = PubackPacket { packet_id: 65535 };
        let encoded = original.encode();

        let mut cursor = Cursor::new(encoded);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let header = runtime.block_on(cursor.read_u8()).unwrap();
        let decoded = runtime.block_on(PubackPacket::read(&mut cursor, header)).unwrap();

        assert_eq!(original.packet_id, decoded.packet_id);
    }
}
