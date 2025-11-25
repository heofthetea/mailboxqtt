use tokio::{
    io::{AsyncWriteExt},
    net::tcp::OwnedWriteHalf as WriteTcpStream,
    sync::mpsc,
};


#[derive(Debug)]
pub enum TcpWriterMsg {
    WritePacket(Vec<u8>),
}

/// Actor to access a TCP stream.
/// A writer should have exclusive ownership of its TCP stream's write half
struct TcpWriter {
    stream: WriteTcpStream,
    receiver: mpsc::UnboundedReceiver<TcpWriterMsg>,
}

impl TcpWriter {
    fn new(stream: WriteTcpStream, receiver: mpsc::UnboundedReceiver<TcpWriterMsg>) -> Self {
        TcpWriter { stream, receiver }
    }

    /// Main event loop of the tcp writer actor
    /// Consecutively works through messages and writes them to the tcp stream
    async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                TcpWriterMsg::WritePacket(packet) => {
                    if let Err(e) = self.stream.write_all(&packet).await {
                        eprintln!("Failed to write packet: {:?}", e);
                        break; // Connection is broken, exit
                    }
                }
            }
        }

        // Graceful shutdown
        let _ = self.stream.shutdown().await;
    }
}

/// Public handle for sending write requests
#[derive(Clone)]
pub struct TcpWriterHandle {
    sender: mpsc::UnboundedSender<TcpWriterMsg>,
}

impl TcpWriterHandle {
    pub fn start(stream: WriteTcpStream) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let actor = TcpWriter::new(stream, rx);

        tokio::spawn(async move {
            actor.run().await;
        });

        TcpWriterHandle { sender: tx }
    }

    // TODO - constructing a vector from a slice - not ideal, but okay for now I just need to get rid of errors
    pub fn write_packet(&self, packet: &[u8]) -> Result<(), mpsc::error::SendError<TcpWriterMsg>> {
        self.sender.send(TcpWriterMsg::WritePacket(Vec::from(packet)))
    }
}
