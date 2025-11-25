use std::io;

use tokio::io::AsyncReadExt;

pub mod connect_packet;
pub mod messages;
pub mod publish_packet;
pub mod subscribe_packet;
pub mod suback_packet;



/// Helper function to read MQTT UTF-8 string (2-byte length prefix)
async fn read_mqtt_string<R: AsyncReadExt + Unpin>(stream: &mut R) -> io::Result<String> {
    let len = stream.read_u16().await? as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;

    String::from_utf8(buf).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, e)
    })
}

/// Helper function to read the MQTT length encoding
#[allow(clippy::similar_names)]
async fn read_remaining_length<R: AsyncReadExt + Unpin>(stream: &mut R) -> io::Result<usize> {
    let mut multiplier = 1;
    let mut value = 0;

    for _ in 0..4 {
        let byte = stream.read_u8().await?;
        value += ((byte & 0x7F) as usize) * multiplier;

        if (byte & 0x80) == 0 {
            return Ok(value);
        }

        multiplier *= 128;
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Malformed remaining length"
    ))
}


/// Helper function to encode MQTT remaining length
fn encode_remaining_length(buffer: &mut Vec<u8>, mut length: usize) {
    loop {
        let mut byte = (length % 128) as u8;
        length /= 128;

        if length > 0 {
            byte |= 0x80; // Set continuation bit
        }

        buffer.push(byte);

        if length == 0 {
            break;
        }
    }
}
