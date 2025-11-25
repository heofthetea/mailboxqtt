use tokio::io::{self, AsyncReadExt};

use super::{read_remaining_length, read_utf8_string};

// todo: understand
#[derive(Debug, Clone)]
#[allow(dead_code)] // we do want to model all mandatory MQTT fields even if we don't use them anywhere
pub struct ConnectPacket {
    pub protocol_name: String,
    pub protocol_level: u8,
    pub client_id: String,
    pub clean_session: bool,
    pub keep_alive: u16,
    // Add more fields as needed
}

impl ConnectPacket {
    pub async fn read_from_stream<R: AsyncReadExt + Unpin>(stream: &mut R) -> io::Result<Self> {
        // Read fixed header
        let packet_type = stream.read_u8().await?;

        if (packet_type >> 4) != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Expected CONNECT packet",
            ));
        }

        let _remaining_length = read_remaining_length(stream).await?;
        let protocol_name = read_utf8_string(stream).await?;
        let protocol_level = stream.read_u8().await?;
        let connect_flags = stream.read_u8().await?;
        let clean_session = (connect_flags & 0b0000_0010) != 0;
        let keep_alive = stream.read_u16().await?;
        let client_id = read_utf8_string(stream).await?;

        // TODO: Read will, username, password based on flags

        Ok(ConnectPacket {
            protocol_name,
            protocol_level,
            client_id,
            clean_session,
            keep_alive,
        })
    }
}
