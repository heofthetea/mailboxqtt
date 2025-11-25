
use super::{encode_remaining_length};

/// MQTT SUBACK packet
///
/// Sent by broker to client in response to a SUBSCRIBE packet.
/// Contains return codes indicating success/failure for each subscription.
///
/// Format:
/// - Fixed Header: packet type (9) + reserved flags (0000)
/// - Variable Header: packet identifier (matching the SUBSCRIBE)
/// - Payload: list of return codes (one per subscription)
#[derive(Debug, Clone)]
pub struct SubackPacket {
    /// Packet identifier (must match the SUBSCRIBE packet)
    pub packet_id: u16,

    /// Return codes for each subscription
    /// 0x00 = Success - Maximum QoS 0
    /// 0x01 = Success - Maximum QoS 1
    /// 0x02 = Success - Maximum QoS 2
    /// 0x80 = Failure
    pub return_codes: Vec<u8>,
}

impl SubackPacket {
    /// Create a SUBACK with all subscriptions successful at QoS 0
    pub fn success_qos0(packet_id: u16, count: usize) -> Self {
        SubackPacket {
            packet_id,
            return_codes: vec![0x00; count],
        }
    }

    /// Encode the SUBACK packet to bytes for sending
    pub fn encode(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        // Fixed header byte: packet type (9) + reserved flags (0000)
        buffer.push(0x90); // 1001_0000

        // Remaining length = packet_id (2 bytes) + return codes
        let remaining_length = 2 + self.return_codes.len();
        encode_remaining_length(&mut buffer, remaining_length);

        // Packet identifier (2 bytes, big-endian)
        buffer.push((self.packet_id >> 8) as u8);
        buffer.push((self.packet_id & 0xFF) as u8);

        // Return codes (one byte per subscription)
        buffer.extend_from_slice(&self.return_codes);

        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_suback_single() {
        let packet = SubackPacket::success_qos0(10, 1);
        let encoded = packet.encode();

        // Check packet type and flags
        assert_eq!(encoded[0], 0x90);

        // Check remaining length (2 + 1 = 3)
        assert_eq!(encoded[1], 0x03);

        // Check packet ID
        assert_eq!(encoded[2], 0x00);
        assert_eq!(encoded[3], 0x0A); // 10

        // Check return code
        assert_eq!(encoded[4], 0x00); // Success QoS 0
    }

    #[test]
    fn test_encode_suback_multiple() {
        let packet = SubackPacket {
            packet_id: 42,
            return_codes: vec![0x00, 0x01, 0x80]
        };
        let encoded = packet.encode();

        // Remaining length should be 2 + 3 = 5
        assert_eq!(encoded[1], 0x05);

        // Check return codes
        assert_eq!(encoded[4], 0x00); // Success QoS 0
        assert_eq!(encoded[5], 0x01); // Success QoS 1
        assert_eq!(encoded[6], 0x80); // Failure
    }
}
