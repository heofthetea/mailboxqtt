use tokio::io::{self, AsyncReadExt};
use crate::protocol::Packet;

use super::{read_utf8_string, read_remaining_length};

/// MQTT SUBSCRIBE packet
///
/// Sent by client to broker to subscribe to one or more topics.
/// The broker responds with SUBACK.
///
/// Format:
/// - Fixed Header: packet type (8) + reserved flags (0010)
/// - Variable Header: packet identifier
/// - Payload: list of topic filters with requested QoS levels
#[derive(Debug, Clone)]
pub struct SubscribePacket {
    /// Packet identifier (required for SUBSCRIBE)
    pub packet_id: u16,

    /// List of topic filters to subscribe to, with requested QoS
    pub subscriptions: Vec<Subscription>,
}

/// A single subscription within a SUBSCRIBE packet
#[derive(Debug, Clone)]
pub struct Subscription {
    /// Topic filter (can include wildcards: + for single level, # for multi-level)
    pub topic_filter: String,

    /// Requested Quality of Service level (0, 1, or 2)
    /// TODO uh how does this work?
    #[allow(dead_code)]
    pub qos: u8,
}

impl Packet for SubscribePacket {
    /// Read a SUBSCRIBE packet from a stream
    /// Assumes reading of the header has happened earlier (for matching the type field), and thus expects it to be passed.
    async fn read<R: AsyncReadExt + Unpin>(stream: &mut R, fixed_header: u8) -> io::Result<Self> {
        let packet_type = fixed_header >> 4;
        if packet_type != 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Expected SUBSCRIBE packet (type 8), got type {}", packet_type)
            ));
        }

        // Check reserved flags (must be 0010 for SUBSCRIBE)
        let flags = fixed_header & 0x0F;
        if flags != 0b0010 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid SUBSCRIBE flags: expected 0010, got {:04b}", flags)
            ));
        }

        // Read remaining length
        let remaining_length = read_remaining_length(stream).await?;

        // Read packet identifier (2 bytes)
        let packet_id = stream.read_u16().await?;

        // Read subscriptions from payload
        let mut subscriptions = Vec::new();
        let mut bytes_read = 2; // Already read packet_id (2 bytes)

        while bytes_read < remaining_length {
            let topic_filter = read_utf8_string(stream).await?;
            bytes_read += 2 + topic_filter.len(); // 2 bytes length + topic bytes

            // Read requested QoS (1 byte)
            let qos = stream.read_u8().await?;
            bytes_read += 1;

            // Validate QoS value
            if qos > 2 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid QoS level: {}", qos)
                ));
            }

            subscriptions.push(Subscription {
                topic_filter,
                qos,
            });
        }

        // SUBSCRIBE must have at least one subscription
        if subscriptions.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SUBSCRIBE packet must contain at least one subscription"
            ));
        }

        Ok(SubscribePacket {
            packet_id,
            subscriptions,
        })
    }

    /// We shouldn't have to write a SUBSCRIBE packet
    fn encode(&self) -> Vec<u8> {
        unimplemented!()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_parse_subscribe_single_topic() {
        // SUBSCRIBE packet with packet_id=10, topic="test/topic", QoS=0
        let data = vec![
            0x82,       // Fixed header: SUBSCRIBE with flags 0010
            0x0F,       // Remaining length: 15
            0x00, 0x0A, // Packet ID: 10
            0x00, 0x0A, // Topic length: 10
            b't', b'e', b's', b't', b'/', b't', b'o', b'p', b'i', b'c',
            0x00,       // QoS: 0
        ];

        let mut cursor = Cursor::new(data);
        let header = cursor.read_u8().await.unwrap();
        let packet = SubscribePacket::read(&mut cursor, header).await.unwrap();

        assert_eq!(packet.packet_id, 10);
        assert_eq!(packet.subscriptions.len(), 1);
        assert_eq!(packet.subscriptions[0].topic_filter, "test/topic");
        assert_eq!(packet.subscriptions[0].qos, 0);
    }

    #[tokio::test]
    async fn test_parse_subscribe_multiple_topics() {
        // SUBSCRIBE with two topics
        let data = vec![
            0x82,       // Fixed header
            0x16,       // Remaining length: 22 (2 + 2+6+1 + 2+8+1)
            0x00, 0x01, // Packet ID: 1
            0x00, 0x06, // Topic 1 length: 6
            b'h', b'o', b'm', b'e', b'/', b'+',
            0x01,       // QoS: 1
            0x00, 0x08, // Topic 2 length: 8
            b's', b'e', b'n', b's', b'o', b'r', b's', b'/',
            0x02,       // QoS: 2
        ];

        let mut cursor = Cursor::new(data);
        let header = cursor.read_u8().await.unwrap();
        let packet = SubscribePacket::read(&mut cursor, header).await.unwrap();

        assert_eq!(packet.packet_id, 1);
        assert_eq!(packet.subscriptions.len(), 2);
        assert_eq!(packet.subscriptions[0].topic_filter, "home/+");
        assert_eq!(packet.subscriptions[0].qos, 1);
        assert_eq!(packet.subscriptions[1].topic_filter, "sensors/");
        assert_eq!(packet.subscriptions[1].qos, 2);
    }
}
