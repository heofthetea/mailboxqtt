use clap::Parser;
use log::{error, info};
use tokio::net::TcpListener;

use crate::{client::Client, message_queue::MessageQueueHandle};

mod client;
mod message_queue;
mod protocol;

#[cfg(test)]
mod tests;

#[derive(Parser)]
struct Args {
    #[arg(env = "SOCKET_ADDR", default_value = "127.0.0.1:1883")]
    address: String
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    
    let args = Args::parse();
    info!("Starting mailboxqtt broker on {}", args.address);

    // Create one shared message queue for the entire broker
    let message_queue = MessageQueueHandle::start();

    // Listen for incoming connections
    let listener = TcpListener::bind(args.address).await?;

    while let Ok((stream, addr)) = listener.accept().await {

        // Clone the message queue handle (cheap operation)
        let mq = message_queue.clone();

        // Spawn a task to handle this client
        tokio::spawn(async move {
            match Client::start(stream, mq).await {
                Ok(client_handle) => {
                    info!("Client {} successfully connected from: {:?} ", client_handle.client_id, addr);
                }
                Err(e) => {
                    error!("Failed to initialize client: {:?}", e);
                }
            }
        });
    }

    Ok(())
}
