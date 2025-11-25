use tokio::io::{self, AsyncReadExt};

use crate::protocol::Packet;

use super::{encode_remaining_length, read_remaining_length, read_utf8_string};

/// MQTT PUBLISH packet
///
/// Used to send messages from client to broker or broker to client.
///
/// Format:
/// - Fixed Header: packet type (3) + flags (DUP, QoS, RETAIN)
/// - Variable Header: topic name + packet identifier (QoS 1/2 only)
/// - Payload: the actual message content
#[derive(Debug, Clone)]
pub struct PublishPacket {
    pub topic: String,
    pub payload: Vec<u8>,

    /// Quality of Service level (for now only 0 and 1)
    pub qos: u8,

    /// Whether this is a retained message
    /// Retained messages are stored by the broker and sent to new subscribers
    pub retain: bool,

    /// Whether this is a duplicate delivery attempt (QoS 1/2 only)
    pub dup: bool,

    /// Packet identifier (only used for QoS 1 and 2)
    /// For QoS 0, this is None
    pub packet_id: Option<u16>,
}

impl Packet for PublishPacket {
    /// Read a PUBLISH packet from a stream
    async fn read<R: AsyncReadExt + Unpin>(stream: &mut R, fixed_header: u8) -> io::Result<Self> {
        // Parse flags from lower 4 bits
        let retain = (fixed_header & 0b0000_0001) != 0;
        let qos = (fixed_header & 0b0000_0110) >> 1;
        let dup = (fixed_header & 0b0000_1000) != 0;

        // Read remaining length
        let remaining_length = read_remaining_length(stream).await?;

        // Read topic name (length-prefixed UTF-8 string)
        let topic = read_utf8_string(stream).await?;

        // Read packet identifier (only for QoS 1 and 2)
        let packet_id = if qos > 0 { Some(stream.read_u16().await?) } else { None };

        // Calculate payload length
        let topic_len = 2 + topic.len(); // 2 bytes for length prefix + topic bytes
        let packet_id_len = if qos > 0 { 2 } else { 0 };
        let payload_len = remaining_length - topic_len - packet_id_len;

        // Read payload
        let mut payload = vec![0u8; payload_len];
        stream.read_exact(&mut payload).await?;

        Ok(PublishPacket {
            topic,
            payload,
            qos,
            retain,
            dup,
            packet_id,
        })
    }

    /// Encode the PUBLISH packet to bytes for sending
    fn encode(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        // Fixed header byte: packet type (3) + flags
        let mut fixed_header = 0b0011_0000; // PUBLISH packet type
        if self.dup {
            fixed_header |= 0b0000_1000;
        }
        fixed_header |= (self.qos & 0b11) << 1;
        if self.retain {
            fixed_header |= 0b0000_0001;
        }
        buffer.push(fixed_header);

        // Calculate remaining length
        let topic_len = 2 + self.topic.len(); // length prefix + topic
        let packet_id_len = if self.qos > 0 { 2 } else { 0 };
        let payload_len = self.payload.len();
        let remaining_length = topic_len + packet_id_len + payload_len;

        // Encode remaining length
        encode_remaining_length(&mut buffer, remaining_length);

        // Encode topic name (length-prefixed)
        buffer.push((self.topic.len() >> 8) as u8);
        buffer.push((self.topic.len() & 0xFF) as u8);
        buffer.extend_from_slice(self.topic.as_bytes());

        // Encode packet identifier (if QoS > 0)
        if let Some(packet_id) = self.packet_id {
            buffer.push((packet_id >> 8) as u8);
            buffer.push((packet_id & 0xFF) as u8);
        }

        // Encode payload
        buffer.extend_from_slice(&self.payload);

        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_publish_qos0() {
        let packet = PublishPacket {
            topic: "test/topic".to_owned(),
            payload: b"hello".to_vec(),
            qos: 0,
            retain: false,
            dup: false,
            packet_id: None,
        };
        let encoded = packet.encode();

        // Check packet type
        assert_eq!(encoded[0] >> 4, 3); // PUBLISH

        // Check flags (QoS 0, no retain, no dup)
        assert_eq!(encoded[0] & 0x0F, 0);
    }

    #[test]
    fn test_encode_remaining_length() {
        let mut buffer = Vec::new();
        encode_remaining_length(&mut buffer, 127);
        assert_eq!(buffer, vec![127]);

        buffer.clear();
        encode_remaining_length(&mut buffer, 128);
        assert_eq!(buffer, vec![0x80, 0x01]);

        buffer.clear();
        encode_remaining_length(&mut buffer, 16383);
        assert_eq!(buffer, vec![0xFF, 0x7F]);
    }
}
