pub mod tcp_writer;

use std::time::Duration;

use tokio::{
    io::{self, AsyncReadExt},
    net::{TcpStream, tcp::OwnedReadHalf},
    sync::mpsc, time::timeout,
};

use crate::{
    client::tcp_writer::TcpWriterHandle,
    message_queue::MessageQueueHandle,
    protocol::{connect_packet::ConnectPacket, messages, publish_packet::PublishPacket, subscribe_packet::SubscribePacket},
};

/// Messages that can be sent to control the client
#[derive(Debug)]
pub enum ClientMessage {
    /// Send a PUBLISH packet to this client
    Publish(PublishPacket),
}

/// Handle for communicating with a client
#[derive(Clone)]
pub struct ClientHandle {
    pub client_id: String,
    sender: mpsc::UnboundedSender<ClientMessage>,
}

impl ClientHandle {
    /// Send a PUBLISH packet to this client
    pub fn publish(&self, packet: PublishPacket) -> Result<(), mpsc::error::SendError<ClientMessage>> {
        self.sender.send(ClientMessage::Publish(packet))
    }
}

/// The actual client state (runs in its own task)
pub struct Client {
    client_id: String,
    reader: OwnedReadHalf,
    writer: TcpWriterHandle,
    mq: MessageQueueHandle,
    receiver: mpsc::UnboundedReceiver<ClientMessage>,
    // When subscribing, we need to create new handles to this Client
    // those handles will obtain a sender by cloning this sender
    _sender: mpsc::UnboundedSender<ClientMessage>,
}

impl Client {
    /// Create and start a new client
    /// Returns a handle for communicating with the created client
    pub async fn start(
        tcp_stream: TcpStream,
        mq: MessageQueueHandle,
    ) -> io::Result<ClientHandle> {
        // Split the stream into read and write halves - thus the client can share one tcp stream
        // with the  `tcp_writer`
        let (mut reader, writer_half) = tcp_stream.into_split();

        // Read the CONNECT packet first
        let connect_packet = ConnectPacket::read_from_stream(&mut reader).await?;
        let client_id = connect_packet.client_id.clone();

        let writer = TcpWriterHandle::start(writer_half);

        // Send CONNACK
        writer
            .write_packet(&messages::CONNACK)
            .map_err(|e| io::Error::other(e))?;
        let (tx, rx) = mpsc::unbounded_channel();

        let client = Client {
            client_id: client_id.clone(),
            reader,
            writer,
            mq,
            receiver: rx,
            _sender: tx.clone(),
        };

        // Sometimes ownership is a pain, the client is moved into the tokio task
        // and so we need to clone this _again_
        let client_id_for_spawn = client.client_id.clone();

        // Start the client's event loop
        tokio::spawn(async move {
            if let Err(e) = client.run().await {
                eprintln!("Client {} error: {:?}", client_id_for_spawn, e);
            }
        });

        Ok(ClientHandle {
            client_id,
            sender: tx,
        })
    }

    /// Main event loop of the client
    /// Handles:
    ///     - Outgoing connections (currently only for PUBLISH)
    ///     - Incoming connections
    /// in this order.
    async fn run(mut self) -> io::Result<()> {
        loop {
            // Try to receive a command (non-blocking check)
            match self.receiver.try_recv() {
                Ok(ClientMessage::Publish(packet)) => {
                    let bytes = packet.encode();
                    if let Err(e) = self.writer.write_packet(&bytes) {
                        eprintln!("Failed to send PUBLISH to client {}: {:?}", self.client_id, e);
                        break;
                    }
                    continue;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // All handles dropped, client should shut down
                    break;
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No commands, continue to read from stream
                }
            }

            // Polling for packets on the stream
            // TODO not sure how to do this better yet, polling is not ideal
            match timeout(Duration::from_millis(100), self.read_next_packet()).await {
                Ok(Ok(Some(()))) => continue,
                Ok(Ok(None)) => {
                    println!("Client {} disconnected", self.client_id);
                    break;
                }
                Ok(Err(e)) => {
                    eprintln!("Client {} read error: {:?}", self.client_id, e);
                    break;
                }
                Err(_) => {
                    // Timeout - loop back to check for commands
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Read and handle the next packet from the client
    /// Returns Ok(Some(())) if packet was handled, Ok(None) if connection closed
    /// and Err(_) when anything fails
    async fn read_next_packet(&mut self) -> io::Result<Option<()>> {
        // Read the first byte to determine packet type
        // The fixed header may contain more information based on the type of packet, so we retain it and pass it
        // to the respective packer parsers
        let fixed_header = match self.reader.read_u8().await {
            Ok(byte) => byte,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        };

        let packet_type = fixed_header >> 4;

        match packet_type {
            3 => {
                // PUBLISH
                let packet = PublishPacket::read_from_stream(&mut self.reader, fixed_header).await?;

                println!("Client {} published to topic '{}'", self.client_id, packet.topic);
                self.mq.publish(packet.topic.clone(), packet);
                Ok(Some(()))
            }
            8 => {
                // SUBSCRIBE
                let SubscribePacket { packet_id, subscriptions} = SubscribePacket::read_from_stream(&mut self.reader, fixed_header).await?;

                println!("Client {} subscribing to {} topics", self.client_id, subscriptions.len());

                for sub in &subscriptions {
                    println!("  - {}", sub.topic_filter);
                    let handle = ClientHandle {
                        client_id: self.client_id.clone(),
                        sender: self._sender.clone(),
                    };
                    self.mq.subscribe(handle, sub.topic_filter.clone());
                }

                // Send SUBACK
                let suback = crate::protocol::suback_packet::SubackPacket::success_qos0(
                    packet_id,
                    subscriptions.len(),
                );
                let suback_bytes = suback.encode();
                self.writer.write_packet(&suback_bytes)
                    .map_err(|e| io::Error::other(e))?;

                Ok(Some(()))
            }
            12 => {
                // PINGREQ
                let _remaining_length = self.reader.read_u8().await?; // Should be 0
                println!("Client {} sent PINGREQ", self.client_id);

                // Send PINGRESP (0xD0, 0x00)
                let pingresp = vec![0xD0, 0x00];
                self.writer.write_packet(&pingresp)
                    .map_err(|e| io::Error::other(e))?;
                Ok(Some(()))
            }
            14 => {
                // DISCONNECT
                let _remaining_length = self.reader.read_u8().await?; // Should be 0
                println!("Client {} sent DISCONNECT", self.client_id);
                Ok(None)
            }
            _ => {
                eprintln!("Client {} sent unknown packet type: {}", self.client_id, packet_type);
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Unknown packet type: {}", packet_type),
                ))
            }
        }
    }
}
