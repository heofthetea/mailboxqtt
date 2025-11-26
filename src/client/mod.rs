pub mod tcp_writer;

use std::{collections::HashMap, time::Duration};

use tokio::{
    io::{self, AsyncReadExt},
    net::{TcpStream, tcp::OwnedReadHalf},
    sync::mpsc,
    time::{Instant, timeout},
};

use crate::{
    client::tcp_writer::TcpWriterHandle,
    message_queue::MessageQueueHandle,
    protocol::{
        Packet, connack::ConnackPacket, connect::ConnectPacket, puback::PubackPacket, publish::PublishPacket,
        subscribe::SubscribePacket,
    },
};

/// Messages that can be sent to control the client
/// Aka: Which packets need to be sent to the client
pub enum ClientCommand {
    /// Send a PUBLISH packet to this client
    Publish((PublishPacket, ClientHandle)),
    /// Send a PUBACK to this client
    Puback(PubackPacket),
}

/// Handle for communicating with a client
#[derive(Clone)]
pub struct ClientHandle {
    pub client_id: String,
    sender: mpsc::UnboundedSender<ClientCommand>,
}

impl ClientHandle {
    /// Send a PUBLISH packet to this client
    pub fn publish(
        &self,
        packet: PublishPacket,
        sender: ClientHandle,
    ) -> Result<(), mpsc::error::SendError<ClientCommand>> {
        self.sender.send(ClientCommand::Publish((packet, sender)))
    }

    pub fn puback(&self, packet_id: u16) -> Result<(), mpsc::error::SendError<ClientCommand>> {
        self.sender.send(ClientCommand::Puback(PubackPacket { packet_id }))
    }

    fn from_client(client: &Client) -> ClientHandle {
        ClientHandle {
            client_id: client.client_id.clone(),
            sender: client._sender.clone(),
        }
    }
}

struct RetainedPacket {
    sender: ClientHandle,
    packet: PublishPacket,
    retry_count: usize,
    last_sent: Instant,
}

/// The actual client state
pub struct Client {
    client_id: String,
    reader: OwnedReadHalf,
    writer: TcpWriterHandle,
    mq: MessageQueueHandle,
    receiver: mpsc::UnboundedReceiver<ClientCommand>,
    // When subscribing, we need to create new handles to this Client
    // those handles will obtain a sender by cloning this sender
    _sender: mpsc::UnboundedSender<ClientCommand>,
    // packet_id -> sender
    pending_acks: HashMap<u16, RetainedPacket>,
    // Periodic task to go over failed messages
    last_retry: Instant,
}

const RETRY_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const MAX_RETRIES: usize = 3;

impl Client {
    /// Create and start a new client
    /// Returns a handle for communicating with the created client
    pub async fn start(tcp_stream: TcpStream, mq: MessageQueueHandle) -> io::Result<ClientHandle> {
        // Split the stream into read and write halves - thus the client can share one tcp stream
        // with the  `tcp_writer`
        let (mut reader, writer_half) = tcp_stream.into_split();

        // Read the CONNECT packet first
        let fixed_header = reader.read_u8().await?;
        let connect_packet = ConnectPacket::read(&mut reader, fixed_header).await?;
        let client_id = connect_packet.client_id.clone();

        let writer = TcpWriterHandle::start(writer_half);

        // Send CONNACK
        let connack = ConnackPacket::accepted();
        writer
            .write_packet(&connack.encode())
            .map_err(|e| io::Error::other(e))?;
        let (tx, rx) = mpsc::unbounded_channel();

        let client = Client {
            client_id: client_id.clone(),
            reader,
            writer,
            mq,
            receiver: rx,
            _sender: tx.clone(),
            pending_acks: HashMap::new(),
            last_retry: Instant::now(),
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

        Ok(ClientHandle { client_id, sender: tx })
    }

    /// Main event loop of the client
    /// Handles:
    ///     - Outgoing connections (currently only for PUBLISH)
    ///     - Incoming connections
    /// in this order.
    async fn run(mut self) -> io::Result<()> {
        loop {
            // Check whether we need to retry packets
            if self.last_retry.elapsed() >= RETRY_CHECK_INTERVAL {
                self.retry_packets()?;
                self.last_retry = Instant::now();
            }
            // when you're trapped in your architecture and need to commit type-theoretical war crimes
            // in order to not redesign the entire architecture
            match self.try_send_packet() {
                // Error printing has already happened below, so only handle the error by killing the client's event loop
                Err(_) => break,
                Ok(()) => {},
            };

            // Polling for packets to read
            // Only give this relatively little time, we don't want to block the loop eternally
            // Receiving a packet also is relatively fast as we don't dispatch network messages in this thread,
            // only call the other client's thread
            match timeout(Duration::from_millis(10), self.try_receive_packet()).await {
                Ok(Ok(Some(()))) => {}
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
            };
        }

        Ok(())
    }

    /// Attempt to resend packets that have not been acknowledged yet
    fn retry_packets(&mut self) -> io::Result<()> {
        for (&packet_id, retained) in self.pending_acks.iter_mut() {
            if retained.retry_count >= MAX_RETRIES {
                eprintln!(
                    "{} failed to ACK packet {} after {} retries, dropped",
                    self.client_id, packet_id, MAX_RETRIES
                );
                _ = self.pending_acks.remove(&packet_id);
                break;
            }
            if retained.last_sent.elapsed() <= RETRY_INTERVAL {
                continue;
            }

            println!(
                "Retrying packet {} to client {} (attempt {}/{})",
                packet_id,
                self.client_id,
                retained.retry_count + 1,
                MAX_RETRIES
            );

            let bytes = retained.packet.encode();
            retained.last_sent = Instant::now();
            retained.retry_count += 1;
            self.writer.write_packet(&bytes).map_err(|e| io::Error::other(e))?;
        }

        Ok(())
    }

    /// Intentionally blocks the client's event loop when a packet is sent
    /// However, this blocking only happens up until the inter-thread communication. Time-costly network traffic is handled
    /// by a different thread
    ///
    /// Returns:
    ///     Err(()) => Something went wrong, "panic" the client loop
    ///     Ok(()) => Continue with the main event loop
    fn try_send_packet(&mut self) -> Result<(), ()> {
        match self.receiver.try_recv() {
            Ok(ClientCommand::Publish((packet, sender))) => {
                match packet.qos {
                    0 => {}
                    1 => {
                        // unwrap is fine because we know packet_id is only None for qos 0
                        let packet_id = packet.packet_id.unwrap();
                        // always overwrites existing entries - if the client is implemented correctly,
                        // an actual overwrite should never exist though
                        self.pending_acks.entry(packet_id).insert_entry(RetainedPacket {
                            sender: sender.clone(),
                            packet: PublishPacket {
                                dup: true,
                                ..packet.clone()
                            },
                            retry_count: 0,
                            last_sent: Instant::now(),
                        });
                        if let Err(e) = sender.puback(packet_id) {
                            println!("Failed to PUBLISH to {}: {:?}", sender.client_id, e);
                            return Err(());
                        }
                        println!("Sent ACK for packet {} to {}", packet_id, sender.client_id);
                    }
                    2 => {
                        // I may never tbh, QoS 1 is enough for us
                        todo!()
                    }
                    3.. => return Ok(()), // invalid qos -> ignore (should be filtered out on read anyway)
                };
                let bytes = packet.encode();
                if let Err(e) = self.writer.write_packet(&bytes) {
                    eprintln!("Failed to send PUBLISH to client {}: {:?}", self.client_id, e);
                    return Err(());
                }
                return Ok(());
            }
            Ok(ClientCommand::Puback(packet)) => {
                let bytes = packet.encode();
                if let Err(e) = self.writer.write_packet(&bytes) {
                    eprintln!("Failed to send PUBACK to client {}: {:?}", self.client_id, e);
                    return Err(());
                }
                println!("Sent puback for {}", packet.packet_id);
                return Ok(());
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // All handles dropped, client should shut down
                return Err(());
            }
            Err(mpsc::error::TryRecvError::Empty) => {
                // No commands, continue to read from stream
            }
        };
        Ok(())
    }

    /// Read and handle the next packet from the client
    /// This is the part of the
    /// Returns Ok(Some(())) if packet was handled, Ok(None) if connection closed and Err(_) when anything fails
    /// Produces side-effects in the sense that it immediately handles received packets by talking to the Message Queue
    async fn try_receive_packet(&mut self) -> io::Result<Option<()>> {
        // Read the first byte to determine packet type
        // The fixed header may contain more information based on the type of packet, so we retain it and pass it
        // to the respective packer parserssender
        let fixed_header = match self.reader.read_u8().await {
            Ok(byte) => byte,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        };

        let packet_type = fixed_header >> 4;

        match packet_type {
            3 => {
                // PUBLISH
                let packet = PublishPacket::read(&mut self.reader, fixed_header).await?;

                println!("Client {} published to topic '{}'", self.client_id, packet.topic);
                // forward the publish packet
                self.mq
                    .publish(packet.topic.clone(), packet, ClientHandle::from_client(self))
                    .await;
                Ok(Some(()))
            }
            4 => {
                // PUBACK
                let packet = PubackPacket::read(&mut self.reader, fixed_header).await?;
                println!(
                    "Client {} acknowledged publish of {}",
                    &self.client_id, &packet.packet_id
                );
                // Remove the packet from the pending acks if it has been acknowledged
                if let Some(retained_entry) = self.pending_acks.remove(&packet.packet_id) {
                    _ = retained_entry.sender.puback(packet.packet_id);
                }

                Ok(Some(()))
            }
            8 => {
                // SUBSCRIBE
                let SubscribePacket {
                    packet_id,
                    subscriptions,
                } = SubscribePacket::read(&mut self.reader, fixed_header).await?;

                println!(
                    "Client {} subscribing to {} topics",
                    self.client_id,
                    subscriptions.len()
                );

                for sub in &subscriptions {
                    println!("  - {}", sub.topic_filter);
                    let handle = ClientHandle::from_client(self);
                    self.mq.subscribe(handle, sub.topic_filter.clone()).await;
                }

                // Send SUBACK
                let suback = crate::protocol::suback::SubackPacket::success_qos0(packet_id, subscriptions.len());
                let suback_bytes = suback.encode();
                self.writer
                    .write_packet(&suback_bytes)
                    .map_err(|e| io::Error::other(e))?;

                Ok(Some(()))
            }
            12 => {
                // PINGREQ
                let _remaining_length = self.reader.read_u8().await?; // Should be 0
                println!("Client {} sent PINGREQ", self.client_id);

                // Send PINGRESP (0xD0, 0x00)
                let pingresp = vec![0xD0, 0x00];
                self.writer.write_packet(&pingresp).map_err(|e| io::Error::other(e))?;
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
