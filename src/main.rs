use clap::Parser;
use tokio::net::TcpListener;

use crate::{client::Client, message_queue::MessageQueueHandle};

mod client;
mod message_queue;
mod protocol;

#[derive(Parser)]
struct Args {
    #[arg(env = "SOCKET_ADDR", default_value = "127.0.0.1:1883")]
    address: String
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    println!("Starting mailboxqtt broker on {}", args.address);

    // Create one shared message queue for the entire broker
    let message_queue = MessageQueueHandle::start();

    // Listen for incoming connections
    let listener = TcpListener::bind(args.address).await?;

    while let Ok((stream, addr)) = listener.accept().await {
        println!("New client connected from: {:?}", addr);

        // Clone the message queue handle (cheap operation)
        let mq = message_queue.clone();

        // Spawn a task to handle this client
        tokio::spawn(async move {
            match Client::start(stream, mq).await {
                Ok(client_handle) => {
                    println!("Client {} successfully connected", client_handle.client_id);
                }
                Err(e) => {
                    eprintln!("Failed to initialize client: {:?}", e);
                }
            }
        });
    }

    Ok(())
}
