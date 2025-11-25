use tokio::io::{self, AsyncReadExt};

use crate::protocol::Packet;

use super::encode_remaining_length;

/// MQTT CONNACK packet
///
/// Sent by broker to client in response to a CONNECT packet.
/// Acknowledges the connection request.
///
/// Format:
/// - Fixed Header: packet type (2) + reserved flags (0000)
/// - Variable Header: Connect Acknowledge Flags + Return Code
/// - Payload: none
#[derive(Debug, Clone)]
pub struct ConnackPacket {
    /// Session Present flag
    /// Indicates whether the server has a stored session for this client
    pub session_present: bool,

    /// Connect Return Code
    /// 0x00 = Connection Accepted
    /// 0x01 = Connection Refused, unacceptable protocol version
    /// 0x02 = Connection Refused, identifier rejected
    /// 0x03 = Connection Refused, Server unavailable
    /// 0x04 = Connection Refused, bad user name or password
    /// 0x05 = Connection Refused, not authorized
    pub return_code: u8,
}

impl ConnackPacket {
    /// Create a CONNACK packet indicating successful connection
    pub fn accepted() -> Self {
        ConnackPacket {
            session_present: false,
            return_code: 0x00,
        }
    }
}

impl Packet for ConnackPacket {
    /// Encode the CONNACK packet to bytes for sending
    fn encode(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        // Fixed header byte: packet type (2) + reserved flags (0000)
        buffer.push(0x20); // 0010_0000

        // Remaining length is always 2
        encode_remaining_length(&mut buffer, 2);

        // Connect Acknowledge Flags (1 byte)
        // Bit 0: Session Present, bits 7-1: reserved (must be 0)
        let ack_flags = if self.session_present { 0x01 } else { 0x00 };
        buffer.push(ack_flags);

        // Connect Return Code (1 byte)
        buffer.push(self.return_code);

        buffer
    }

    /// We shouldn't have to read a CONNACK packet (broker only sends them)
    #[allow(dead_code)]
    async fn read<R>(_stream: &mut R, _fixed_header: u8) -> io::Result<Self>
    where
        R: AsyncReadExt + Unpin,
        Self: Sized,
    {
        unimplemented!("Broker does not read CONNACK packets")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ConnackPacket {
        /// Create a CONNACK packet with a specific return code
        pub fn with_return_code(return_code: u8, session_present: bool) -> Self {
            ConnackPacket {
                session_present,
                return_code,
            }
        }
    }

    #[test]
    fn test_encode_connack_accepted() {
        let packet = ConnackPacket::accepted();
        let encoded = packet.encode();

        // Check packet type and flags
        assert_eq!(encoded[0], 0x20); // 0010_0000

        // Check remaining length (always 2)
        assert_eq!(encoded[1], 0x02);

        // Check acknowledge flags (session present = false)
        assert_eq!(encoded[2], 0x00);

        // Check return code (connection accepted)
        assert_eq!(encoded[3], 0x00);
    }

    #[test]
    fn test_encode_connack_with_session() {
        let packet = ConnackPacket::with_return_code(0x00, true);
        let encoded = packet.encode();

        // Check acknowledge flags (session present = true)
        assert_eq!(encoded[2], 0x01);

        // Check return code (connection accepted)
        assert_eq!(encoded[3], 0x00);
    }

    #[test]
    fn test_encode_connack_refused() {
        let packet = ConnackPacket::with_return_code(0x05, false);
        let encoded = packet.encode();

        // Check return code (not authorized)
        assert_eq!(encoded[3], 0x05);
    }
}
