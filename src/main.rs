use tokio::net::TcpListener;

use crate::{client::Client, message_queue::MessageQueueHandle};

mod client;
mod message_queue;
mod protocol;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    println!("Starting mailboxqtt broker on 127.0.0.1:1883");

    // Create one shared message queue for the entire broker
    let message_queue = MessageQueueHandle::start();

    // Listen for incoming connections
    let listener = TcpListener::bind("127.0.0.1:1883").await?;

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
