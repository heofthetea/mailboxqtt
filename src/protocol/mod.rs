/// This module contains structs for dealing with the en- and decoding of MQTT packets.
use std::io;

use tokio::io::AsyncReadExt;

use crate::protocol::publish::PublishPacket;

pub mod connack;
pub mod connect;
pub mod puback;
pub mod publish;
pub mod suback;
pub mod subscribe;

///////////////////////////////////////////////////////// TYPES ////////////////////////////////////////////////////////

/// A packet that can be sent or received by the broker.
/// Note that if a packet must (a) never be read or (b) never be written by the broker,
/// The corresponding methods ((a) `read` and (b) `encode`) may be left `unimplemented()`.
pub trait Packet {
    /// Read the Packet from `stream`.
    /// This assumes that the MQTT fixed header has been read from the stream beforehand to pattern-match the type of packet,
    /// meaning it gets passed as the `fixed_header` parameter
    async fn read<R>(stream: &mut R, fixed_header: u8) -> io::Result<Self>
    where
        R: AsyncReadExt + Unpin,
        Self: Sized;

    /// Encode the Packet into a byte-vector to write in order to write it onto a stream.
    fn encode(&self) -> Vec<u8>;
}

/////////////////////////////////////////////// HELPER PARSING FUNCTIONS ///////////////////////////////////////////////

/// Read an mqtt utf8 string (2 bytes) from a stream
/// mqtt first tells us how many characters the string will be (`len`), then we just read it into a buffer
async fn read_utf8_string<R>(stream: &mut R) -> io::Result<String>
where
    R: AsyncReadExt + Unpin,
{
    let len = stream.read_u16().await? as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;

    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Read the MQTT length encoding
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
        "Malformed remaining length",
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
